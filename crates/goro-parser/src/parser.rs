use crate::ast::*;
use crate::token::{Span, Token, TokenKind};

/// Parse error
#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Parse error on line {}: {}",
            self.span.line, self.message
        )
    }
}

impl std::error::Error for ParseError {}

pub type ParseResult<T> = Result<T, ParseError>;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    depth: u32,
    /// Anonymous class declarations that need to be emitted before the current statement
    anon_class_stmts: Vec<Statement>,
    /// Counter for generating unique anonymous class names
    anon_counter: u32,
    /// Set to true by parse_arguments() when it detects first-class callable syntax (...)
    first_class_callable_flag: bool,
    /// Pending extra constants from comma-separated const declarations
    pending_extra_constants: Vec<ClassMember>,
    /// Pending extra properties from comma-separated property declarations
    pending_extra_properties: Vec<ClassMember>,
}

const MAX_PARSE_DEPTH: u32 = 512;

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            depth: 0,
            anon_class_stmts: Vec::new(),
            anon_counter: 0,
            first_class_callable_flag: false,
            pending_extra_constants: Vec::new(),
            pending_extra_properties: Vec::new(),
        }
    }

    fn enter_depth(&mut self) -> ParseResult<()> {
        self.depth += 1;
        if self.depth > MAX_PARSE_DEPTH {
            Err(ParseError {
                message: "Maximum nesting depth exceeded".into(),
                span: self.current().span,
            })
        } else {
            Ok(())
        }
    }

    fn leave_depth(&mut self) {
        self.depth -= 1;
    }

    fn current(&self) -> &Token {
        self.tokens
            .get(self.pos)
            .unwrap_or(self.tokens.last().unwrap())
    }

    fn peek(&self) -> &TokenKind {
        &self.current().kind
    }

    fn peek_at(&self, offset: usize) -> &TokenKind {
        let idx = (self.pos + offset).min(self.tokens.len() - 1);
        &self.tokens[idx].kind
    }

    fn span(&self) -> Span {
        self.current().span
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos.min(self.tokens.len() - 1)];
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, expected: &TokenKind) -> ParseResult<Span> {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(expected) {
            let span = self.span();
            self.advance();
            Ok(span)
        } else {
            Err(ParseError {
                message: format!("syntax error, unexpected {}, expecting {}", self.peek().to_php_unexpected(), expected.to_php_name()),
                span: self.span(),
            })
        }
    }

    fn expect_semicolon(&mut self) -> ParseResult<()> {
        // PHP allows ?> to act as a semicolon
        match self.peek() {
            TokenKind::Semicolon => {
                self.advance();
                Ok(())
            }
            TokenKind::CloseTag => {
                // Don't consume the close tag - it will be handled by the statement loop
                Ok(())
            }
            _ => Err(ParseError {
                message: format!("syntax error, unexpected {}", self.peek().to_php_unexpected()),
                span: self.span(),
            }),
        }
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn is_at_end(&self) -> bool {
        matches!(self.peek(), TokenKind::Eof)
    }

    /// Parse a complete PHP program
    pub fn parse(&mut self) -> ParseResult<Program> {
        let mut statements = Vec::new();
        while !self.is_at_end() {
            let stmt = self.parse_statement()?;
            // Drain any anonymous class declarations collected during expression parsing
            if !self.anon_class_stmts.is_empty() {
                let anon_stmts: Vec<_> = self.anon_class_stmts.drain(..).collect();
                for anon_stmt in anon_stmts {
                    statements.push(anon_stmt);
                }
            }
            statements.push(stmt);
        }
        Ok(Program { statements })
    }

    fn parse_attributes(&mut self) -> ParseResult<Vec<Attribute>> {
        let mut attrs = Vec::new();
        while matches!(self.peek(), TokenKind::AttributeOpen) {
            self.advance();
            loop {
                let mut parts = Vec::new();
                if matches!(self.peek(), TokenKind::Backslash) { self.advance(); }
                match self.peek().clone() { TokenKind::Identifier(n) => { parts.push(n); self.advance(); } _ if self.is_semi_reserved_keyword() => { parts.push(self.keyword_to_identifier()); self.advance(); } _ => { return Err(ParseError { message: "expected attribute name".into(), span: self.span() }); } }
                while matches!(self.peek(), TokenKind::Backslash) { self.advance(); match self.peek().clone() { TokenKind::Identifier(n) => { parts.push(n); self.advance(); } _ if self.is_semi_reserved_keyword() => { parts.push(self.keyword_to_identifier()); self.advance(); } _ => break, } }
                let args = if matches!(self.peek(), TokenKind::OpenParen) { self.advance(); let a = self.parse_arguments()?; self.expect(&TokenKind::CloseParen)?; a } else { Vec::new() };
                attrs.push(Attribute { name: parts, args });
                if !self.eat(&TokenKind::Comma) { break; }
                if matches!(self.peek(), TokenKind::CloseBracket) { break; }
            }
            self.expect(&TokenKind::CloseBracket)?;
        }
        Ok(attrs)
    }
    fn parse_statement(&mut self) -> ParseResult<Statement> {
        let span = self.span();
        match self.peek().clone() {
            TokenKind::InlineHtml(html) => {
                self.advance();
                Ok(Statement {
                    kind: StmtKind::InlineHtml(html),
                    span,
                })
            }
            TokenKind::OpenTag | TokenKind::OpenTagShort => {
                let is_short = matches!(self.peek(), TokenKind::OpenTagShort);
                self.advance();
                if is_short {
                    // <?= is equivalent to <?php echo
                    let expr = self.parse_expression()?;
                    self.expect_semicolon()?;
                    Ok(Statement {
                        kind: StmtKind::Echo(vec![expr]),
                        span,
                    })
                } else {
                    // <?php - just continue parsing
                    self.parse_statement()
                }
            }
            TokenKind::CloseTag => {
                self.advance();
                // ?> goes back to HTML mode. The next token might be InlineHtml or another OpenTag.
                Ok(Statement {
                    kind: StmtKind::Nop,
                    span,
                })
            }
            TokenKind::Echo => {
                self.advance();
                let mut exprs = vec![self.parse_expression()?];
                while self.eat(&TokenKind::Comma) {
                    exprs.push(self.parse_expression()?);
                }
                self.expect_semicolon()?;
                Ok(Statement {
                    kind: StmtKind::Echo(exprs),
                    span,
                })
            }
            TokenKind::Return => {
                self.advance();
                let value = if matches!(self.peek(), TokenKind::Semicolon | TokenKind::CloseTag) {
                    None
                } else {
                    Some(self.parse_expression()?)
                };
                self.expect_semicolon()?;
                Ok(Statement {
                    kind: StmtKind::Return(value),
                    span,
                })
            }
            TokenKind::If => self.parse_if(),
            TokenKind::While => self.parse_while(),
            TokenKind::Do => self.parse_do_while(),
            TokenKind::For => self.parse_for(),
            TokenKind::Foreach => self.parse_foreach(),
            TokenKind::Switch => self.parse_switch(),
            TokenKind::Break => {
                self.advance();
                let depth = if matches!(self.peek(), TokenKind::Semicolon | TokenKind::CloseTag) {
                    None
                } else {
                    Some(self.parse_expression()?)
                };
                self.expect_semicolon()?;
                Ok(Statement {
                    kind: StmtKind::Break(depth),
                    span,
                })
            }
            TokenKind::Continue => {
                self.advance();
                let depth = if matches!(self.peek(), TokenKind::Semicolon | TokenKind::CloseTag) {
                    None
                } else {
                    Some(self.parse_expression()?)
                };
                self.expect_semicolon()?;
                Ok(Statement {
                    kind: StmtKind::Continue(depth),
                    span,
                })
            }
            TokenKind::Static
                if matches!(
                    self.tokens.get(self.pos + 1).map(|t| &t.kind),
                    Some(TokenKind::Variable(_))
                ) =>
            {
                // static $var = expr;
                self.advance(); // consume 'static'
                let mut vars = Vec::new();
                loop {
                    let name = match self.peek().clone() {
                        TokenKind::Variable(name) => {
                            self.advance();
                            name
                        }
                        _ => {
                            return Err(ParseError {
                                message: "expected variable after 'static'".into(),
                                span: self.span(),
                            });
                        }
                    };
                    let default = if self.eat(&TokenKind::Assign) {
                        Some(self.parse_expression()?)
                    } else {
                        None
                    };
                    vars.push((name, default));
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect_semicolon()?;
                Ok(Statement {
                    kind: StmtKind::StaticVar(vars),
                    span,
                })
            }
            TokenKind::AttributeOpen => { let attributes = self.parse_attributes()?; match self.peek() { TokenKind::Function => self.parse_function_decl_with_attrs(attributes), TokenKind::Class | TokenKind::Abstract | TokenKind::Final | TokenKind::Interface | TokenKind::Trait | TokenKind::Enum => self.parse_class_decl_with_attrs(attributes), TokenKind::Readonly if matches!(self.peek_at(1), TokenKind::Class | TokenKind::Enum | TokenKind::Abstract | TokenKind::Final | TokenKind::Function | TokenKind::Trait | TokenKind::Interface | TokenKind::Readonly) => self.parse_class_decl_with_attrs(attributes), _ => self.parse_statement(), } }
            TokenKind::Function => self.parse_function_decl_with_attrs(Vec::new()),
            TokenKind::Class
            | TokenKind::Abstract
            | TokenKind::Final
            | TokenKind::Interface
            | TokenKind::Trait
            | TokenKind::Enum => self.parse_class_decl_with_attrs(Vec::new()),
            TokenKind::Readonly => {
                // readonly can be a class modifier (readonly class Foo {}) or a function call
                // Check if followed by class/enum/abstract/final/function
                if matches!(self.peek_at(1), TokenKind::Class | TokenKind::Enum | TokenKind::Abstract | TokenKind::Final | TokenKind::Function | TokenKind::Trait | TokenKind::Interface | TokenKind::Readonly) {
                    self.parse_class_decl_with_attrs(Vec::new())
                } else {
                    // Treat 'readonly' as identifier for function call etc.
                    self.advance();
                    let name = b"readonly".to_vec();
                    if matches!(self.peek(), TokenKind::OpenParen) {
                        // Function call: readonly()
                        self.advance();
                        let args = self.parse_arguments()?;
                        let is_fcc = self.first_class_callable_flag;
                        let end_span = self.span();
                        self.expect(&TokenKind::CloseParen)?;
                        let name_expr = Expr {
                            kind: ExprKind::Identifier(name),
                            span,
                        };
                        let expr = if is_fcc {
                            Expr {
                                span: span.merge(end_span),
                                kind: ExprKind::FirstClassCallable(CallableTarget::Function(Box::new(name_expr))),
                            }
                        } else {
                            Expr {
                                span: span.merge(end_span),
                                kind: ExprKind::FunctionCall {
                                    name: Box::new(name_expr),
                                    args,
                                },
                            }
                        };
                        self.expect_semicolon()?;
                        Ok(Statement {
                            kind: StmtKind::Expression(expr),
                            span,
                        })
                    } else {
                        // Other readonly usage as identifier
                        let expr = Expr {
                            kind: ExprKind::Identifier(name),
                            span,
                        };
                        self.expect_semicolon()?;
                        Ok(Statement {
                            kind: StmtKind::Expression(expr),
                            span,
                        })
                    }
                }
            }
            TokenKind::Const => {
                // const FOO = value;
                self.advance();
                let name = match self.peek().clone() {
                    TokenKind::Identifier(name) => {
                        self.advance();
                        name
                    }
                    _ if self.is_semi_reserved_keyword() => {
                        let kw = self.keyword_to_identifier();
                        self.advance();
                        kw
                    }
                    _ => {
                        return Err(ParseError {
                            message: "expected constant name".into(),
                            span: self.span(),
                        });
                    }
                };
                self.expect(&TokenKind::Assign)?;
                let value = self.parse_expression()?;
                self.expect_semicolon()?;
                // Treat as a declare directive
                Ok(Statement {
                    kind: StmtKind::Declare {
                        directives: vec![(name, value)],
                        body: None,
                    },
                    span,
                })
            }
            TokenKind::Namespace => {
                self.advance();
                // namespace Name\Space;  or  namespace Name\Space { ... }
                // Namespace names can contain reserved keywords as parts
                let mut name_parts = Vec::new();
                if matches!(self.peek(), TokenKind::Identifier(_)) || self.is_semi_reserved_keyword() || matches!(self.peek(), TokenKind::Fn | TokenKind::Match | TokenKind::Null | TokenKind::True | TokenKind::False) {
                    loop {
                        match self.peek().clone() {
                            TokenKind::Identifier(part) => {
                                self.advance();
                                name_parts.push(part);
                            }
                            _ if self.is_semi_reserved_keyword() || matches!(self.peek(), TokenKind::Fn | TokenKind::Match | TokenKind::Null | TokenKind::True | TokenKind::False) => {
                                let kw = self.keyword_to_identifier();
                                self.advance();
                                name_parts.push(kw);
                            }
                            _ => break,
                        }
                        if !self.eat(&TokenKind::Backslash) {
                            break;
                        }
                    }
                }
                let body = if matches!(self.peek(), TokenKind::OpenBrace) {
                    let stmts = self.parse_block()?;
                    Some(stmts)
                } else {
                    self.expect_semicolon()?;
                    None
                };
                Ok(Statement {
                    kind: StmtKind::NamespaceDecl {
                        name: if name_parts.is_empty() {
                            None
                        } else {
                            Some(name_parts)
                        },
                        body,
                    },
                    span,
                })
            }
            TokenKind::Use => {
                self.advance();
                // Check for `use function` or `use const`
                let default_kind = match self.peek() {
                    TokenKind::Function => {
                        self.advance();
                        UseKind::Function
                    }
                    TokenKind::Const => {
                        self.advance();
                        UseKind::Constant
                    }
                    _ => UseKind::Normal,
                };

                let mut items = Vec::new();

                // Parse the first name (could be a prefix for group use)
                let mut first_parts = Vec::new();
                // Handle optional leading backslash
                self.eat(&TokenKind::Backslash);
                loop {
                    match self.peek().clone() {
                        TokenKind::Identifier(part) => {
                            self.advance();
                            first_parts.push(part);
                        }
                        _ if self.is_semi_reserved_keyword() => {
                            first_parts.push(self.keyword_to_identifier());
                            self.advance();
                        }
                        _ => break,
                    }
                    if !self.eat(&TokenKind::Backslash) {
                        break;
                    }
                }

                if matches!(self.peek(), TokenKind::OpenBrace) {
                    // Group use: use Foo\Bar\{Baz, Qux as Q};
                    // first_parts is the prefix (e.g., ["Foo", "Bar"])
                    self.advance(); // eat {
                    loop {
                        if matches!(self.peek(), TokenKind::CloseBrace) {
                            break;
                        }
                        // Check for `function` or `const` per-item kind
                        let item_kind = match self.peek() {
                            TokenKind::Function => {
                                // `function` keyword as per-item use kind
                                self.advance();
                                UseKind::Function
                            }
                            TokenKind::Const => {
                                // `const` keyword as per-item use kind
                                self.advance();
                                UseKind::Constant
                            }
                            _ => default_kind,
                        };
                        let mut name_parts = first_parts.clone();
                        loop {
                            match self.peek().clone() {
                                TokenKind::Identifier(part) => {
                                    self.advance();
                                    name_parts.push(part);
                                }
                                _ if self.is_semi_reserved_keyword() => {
                                    name_parts.push(self.keyword_to_identifier());
                                    self.advance();
                                }
                                _ => break,
                            }
                            if !self.eat(&TokenKind::Backslash) {
                                break;
                            }
                        }
                        let alias = if self.eat(&TokenKind::As) {
                            match self.peek().clone() {
                                TokenKind::Identifier(a) => {
                                    self.advance();
                                    Some(a)
                                }
                                _ if self.is_semi_reserved_keyword() => {
                                    let kw = self.keyword_to_identifier();
                                    self.advance();
                                    Some(kw)
                                }
                                _ => None,
                            }
                        } else {
                            None
                        };
                        items.push(UseItem {
                            name: name_parts,
                            alias,
                            kind: item_kind,
                        });
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(&TokenKind::CloseBrace)?;
                } else {
                    // Simple use: use Foo\Bar\Baz as Alias;
                    // or multiple: use Foo\Bar, Baz\Qux;
                    let alias = if self.eat(&TokenKind::As) {
                        match self.peek().clone() {
                            TokenKind::Identifier(a) => {
                                self.advance();
                                Some(a)
                            }
                            _ if self.is_semi_reserved_keyword() => {
                                let kw = self.keyword_to_identifier();
                                self.advance();
                                Some(kw)
                            }
                            _ => None,
                        }
                    } else {
                        None
                    };
                    items.push(UseItem {
                        name: first_parts,
                        alias,
                        kind: default_kind,
                    });
                    // Parse additional comma-separated use items
                    while self.eat(&TokenKind::Comma) {
                        let mut name_parts = Vec::new();
                        self.eat(&TokenKind::Backslash);
                        loop {
                            match self.peek().clone() {
                                TokenKind::Identifier(part) => {
                                    self.advance();
                                    name_parts.push(part);
                                }
                                _ if self.is_semi_reserved_keyword() => {
                                    name_parts.push(self.keyword_to_identifier());
                                    self.advance();
                                }
                                _ => break,
                            }
                            if !self.eat(&TokenKind::Backslash) {
                                break;
                            }
                        }
                        let alias = if self.eat(&TokenKind::As) {
                            match self.peek().clone() {
                                TokenKind::Identifier(a) => {
                                    self.advance();
                                    Some(a)
                                }
                                _ if self.is_semi_reserved_keyword() => {
                                    let kw = self.keyword_to_identifier();
                                    self.advance();
                                    Some(kw)
                                }
                                _ => None,
                            }
                        } else {
                            None
                        };
                        items.push(UseItem {
                            name: name_parts,
                            alias,
                            kind: default_kind,
                        });
                    }
                }
                self.expect_semicolon()?;
                Ok(Statement {
                    kind: StmtKind::UseDecl(items),
                    span,
                })
            }
            TokenKind::Try => self.parse_try_catch(),
            TokenKind::Throw => {
                self.advance();
                let expr = self.parse_expression()?;
                self.expect_semicolon()?;
                Ok(Statement {
                    kind: StmtKind::Throw(expr),
                    span,
                })
            }
            TokenKind::Global => {
                self.advance();
                let mut vars = Vec::new();
                loop {
                    if let TokenKind::Variable(name) = self.peek().clone() {
                        self.advance();
                        vars.push(name);
                    } else {
                        return Err(ParseError {
                            message: "expected variable name after 'global'".into(),
                            span: self.span(),
                        });
                    }
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect_semicolon()?;
                Ok(Statement {
                    kind: StmtKind::Global(vars),
                    span,
                })
            }
            TokenKind::Unset => {
                self.advance();
                self.expect(&TokenKind::OpenParen)?;
                let mut exprs = vec![self.parse_expression()?];
                while self.eat(&TokenKind::Comma) {
                    exprs.push(self.parse_expression()?);
                }
                self.expect(&TokenKind::CloseParen)?;
                self.expect_semicolon()?;
                Ok(Statement {
                    kind: StmtKind::Unset(exprs),
                    span,
                })
            }
            TokenKind::Declare => self.parse_declare(),
            TokenKind::Semicolon => {
                self.advance();
                Ok(Statement {
                    kind: StmtKind::Nop,
                    span,
                })
            }
            TokenKind::OpenBrace => {
                // Block statement - parse as multiple statements, wrap in a synthetic block
                self.advance();
                let mut stmts = Vec::new();
                while !matches!(self.peek(), TokenKind::CloseBrace | TokenKind::Eof) {
                    stmts.push(self.parse_statement()?);
                }
                self.expect(&TokenKind::CloseBrace)?;
                // Return statements inline (PHP blocks are not scopes)
                if stmts.len() == 1 {
                    Ok(stmts.into_iter().next().unwrap())
                } else {
                    // Wrap in an if(true) as a hack, or we could add a Block variant
                    // For now, just return the first or a nop
                    Ok(Statement {
                        kind: StmtKind::If {
                            condition: Expr {
                                kind: ExprKind::True,
                                span,
                            },
                            body: stmts,
                            elseif_clauses: vec![],
                            else_body: None,
                        },
                        span,
                    })
                }
            }
            TokenKind::Goto => {
                self.advance();
                if let TokenKind::Identifier(label) = self.peek().clone() {
                    self.advance();
                    self.expect_semicolon()?;
                    Ok(Statement {
                        kind: StmtKind::Goto(label),
                        span,
                    })
                } else {
                    Err(ParseError {
                        message: "expected label name after 'goto'".into(),
                        span: self.span(),
                    })
                }
            }
            _ => {
                // Check for __halt_compiler()
                if let TokenKind::Identifier(ref name) = self.peek().clone() {
                    if name.eq_ignore_ascii_case(b"__halt_compiler") {
                        // Check for () and ;
                        if matches!(self.peek_at(1), TokenKind::OpenParen)
                            && matches!(self.peek_at(2), TokenKind::CloseParen)
                            && matches!(self.peek_at(3), TokenKind::Semicolon) {
                            self.advance(); // __halt_compiler
                            self.advance(); // (
                            self.advance(); // )
                            self.advance(); // ;
                            // Skip all remaining tokens until EOF
                            while !self.is_at_end() {
                                self.advance();
                            }
                            return Ok(Statement {
                                kind: StmtKind::Nop,
                                span,
                            });
                        }
                    }
                }
                // Check for label: identifier followed by colon (but not ::)
                if let TokenKind::Identifier(name) = self.peek().clone() {
                    if self.tokens.get(self.pos + 1).map(|t| &t.kind) == Some(&TokenKind::Colon) {
                        // Make sure it's not part of a ternary or other construct
                        // Labels appear at statement level as "label:" on their own
                        self.advance(); // consume identifier
                        self.advance(); // consume colon
                        return Ok(Statement {
                            kind: StmtKind::Label(name),
                            span,
                        });
                    }
                }
                // (void) cast as statement (PHP 8.5): evaluates and discards the expression
                if matches!(self.peek(), TokenKind::VoidCast) {
                    self.advance();
                    let operand = self.parse_expression()?;
                    self.expect_semicolon()?;
                    return Ok(Statement {
                        kind: StmtKind::Expression(Expr {
                            span: span.merge(operand.span),
                            kind: ExprKind::Cast(CastType::Void, Box::new(operand)),
                        }),
                        span,
                    });
                }
                // Expression statement
                let expr = self.parse_expression()?;
                self.expect_semicolon()?;
                Ok(Statement {
                    kind: StmtKind::Expression(expr),
                    span,
                })
            }
        }
    }

    fn parse_block(&mut self) -> ParseResult<Vec<Statement>> {
        self.expect(&TokenKind::OpenBrace)?;
        let mut stmts = Vec::new();
        while !matches!(self.peek(), TokenKind::CloseBrace | TokenKind::Eof) {
            stmts.push(self.parse_statement()?);
        }
        self.expect(&TokenKind::CloseBrace)?;
        Ok(stmts)
    }

    fn parse_if(&mut self) -> ParseResult<Statement> {
        let span = self.span();
        self.advance(); // consume 'if'
        self.expect(&TokenKind::OpenParen)?;
        let condition = self.parse_expression()?;
        self.expect(&TokenKind::CloseParen)?;

        // Check for alternative syntax: if (...):
        if self.eat(&TokenKind::Colon) {
            let mut body = Vec::new();
            while !matches!(
                self.peek(),
                TokenKind::ElseIf | TokenKind::Else | TokenKind::EndIf | TokenKind::Eof
            ) {
                body.push(self.parse_statement()?);
            }

            let mut elseif_clauses = Vec::new();
            while self.eat(&TokenKind::ElseIf) {
                self.expect(&TokenKind::OpenParen)?;
                let cond = self.parse_expression()?;
                self.expect(&TokenKind::CloseParen)?;
                self.expect(&TokenKind::Colon)?;
                let mut elsif_body = Vec::new();
                while !matches!(
                    self.peek(),
                    TokenKind::ElseIf | TokenKind::Else | TokenKind::EndIf | TokenKind::Eof
                ) {
                    elsif_body.push(self.parse_statement()?);
                }
                elseif_clauses.push((cond, elsif_body));
            }

            let else_body = if self.eat(&TokenKind::Else) {
                self.expect(&TokenKind::Colon)?;
                let mut else_stmts = Vec::new();
                while !matches!(self.peek(), TokenKind::EndIf | TokenKind::Eof) {
                    else_stmts.push(self.parse_statement()?);
                }
                Some(else_stmts)
            } else {
                None
            };

            self.expect(&TokenKind::EndIf)?;
            self.expect_semicolon()?;

            return Ok(Statement {
                kind: StmtKind::If {
                    condition,
                    body,
                    elseif_clauses,
                    else_body,
                },
                span,
            });
        }

        let body = if matches!(self.peek(), TokenKind::OpenBrace) {
            self.parse_block()?
        } else {
            vec![self.parse_statement()?]
        };

        let mut elseif_clauses = Vec::new();
        while self.eat(&TokenKind::ElseIf) {
            self.expect(&TokenKind::OpenParen)?;
            let cond = self.parse_expression()?;
            self.expect(&TokenKind::CloseParen)?;
            let elsif_body = if matches!(self.peek(), TokenKind::OpenBrace) {
                self.parse_block()?
            } else {
                vec![self.parse_statement()?]
            };
            elseif_clauses.push((cond, elsif_body));
        }

        let else_body = if self.eat(&TokenKind::Else) {
            if matches!(self.peek(), TokenKind::OpenBrace) {
                Some(self.parse_block()?)
            } else {
                Some(vec![self.parse_statement()?])
            }
        } else {
            None
        };

        Ok(Statement {
            kind: StmtKind::If {
                condition,
                body,
                elseif_clauses,
                else_body,
            },
            span,
        })
    }

    fn parse_while(&mut self) -> ParseResult<Statement> {
        let span = self.span();
        self.advance();
        self.expect(&TokenKind::OpenParen)?;
        let condition = self.parse_expression()?;
        self.expect(&TokenKind::CloseParen)?;

        let body = if matches!(self.peek(), TokenKind::OpenBrace) {
            self.parse_block()?
        } else if self.eat(&TokenKind::Colon) {
            // Alternative syntax: while (): ... endwhile;
            let mut stmts = Vec::new();
            while !matches!(self.peek(), TokenKind::EndWhile | TokenKind::Eof) {
                stmts.push(self.parse_statement()?);
            }
            self.expect(&TokenKind::EndWhile)?;
            self.expect_semicolon()?;
            stmts
        } else {
            vec![self.parse_statement()?]
        };

        Ok(Statement {
            kind: StmtKind::While { condition, body },
            span,
        })
    }

    fn parse_do_while(&mut self) -> ParseResult<Statement> {
        let span = self.span();
        self.advance(); // do
        let body = self.parse_block()?;
        self.expect(&TokenKind::While)?;
        self.expect(&TokenKind::OpenParen)?;
        let condition = self.parse_expression()?;
        self.expect(&TokenKind::CloseParen)?;
        self.expect_semicolon()?;
        Ok(Statement {
            kind: StmtKind::DoWhile { body, condition },
            span,
        })
    }

    fn parse_for(&mut self) -> ParseResult<Statement> {
        let span = self.span();
        self.advance(); // for
        self.expect(&TokenKind::OpenParen)?;

        let init = self.parse_expression_list(&TokenKind::Semicolon)?;
        self.expect(&TokenKind::Semicolon)?;
        let condition = self.parse_expression_list(&TokenKind::Semicolon)?;
        self.expect(&TokenKind::Semicolon)?;
        let update = self.parse_expression_list(&TokenKind::CloseParen)?;
        self.expect(&TokenKind::CloseParen)?;

        let body = if matches!(self.peek(), TokenKind::OpenBrace) {
            self.parse_block()?
        } else if self.eat(&TokenKind::Colon) {
            // Alternative syntax: for (): ... endfor;
            let mut stmts = Vec::new();
            while !matches!(self.peek(), TokenKind::EndFor | TokenKind::Eof) {
                stmts.push(self.parse_statement()?);
            }
            self.expect(&TokenKind::EndFor)?;
            self.expect_semicolon()?;
            stmts
        } else {
            vec![self.parse_statement()?]
        };

        Ok(Statement {
            kind: StmtKind::For {
                init,
                condition,
                update,
                body,
            },
            span,
        })
    }

    fn parse_expression_list(&mut self, terminator: &TokenKind) -> ParseResult<Vec<Expr>> {
        let mut exprs = Vec::new();
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(terminator) {
            return Ok(exprs);
        }
        // Handle (void)expr in expression lists (e.g., for loop update)
        if matches!(self.peek(), TokenKind::VoidCast) {
            let span = self.span();
            self.advance();
            let operand = self.parse_expression()?;
            exprs.push(Expr {
                span: span.merge(operand.span),
                kind: ExprKind::Cast(CastType::Void, Box::new(operand)),
            });
        } else {
            exprs.push(self.parse_expression()?);
        }
        while self.eat(&TokenKind::Comma) {
            if matches!(self.peek(), TokenKind::VoidCast) {
                let span = self.span();
                self.advance();
                let operand = self.parse_expression()?;
                exprs.push(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::Cast(CastType::Void, Box::new(operand)),
                });
            } else {
                exprs.push(self.parse_expression()?);
            }
        }
        Ok(exprs)
    }

    fn parse_foreach(&mut self) -> ParseResult<Statement> {
        let span = self.span();
        self.advance(); // foreach
        self.expect(&TokenKind::OpenParen)?;
        let expr = self.parse_expression()?;
        self.expect(&TokenKind::As)?;

        let by_ref = self.eat(&TokenKind::Ampersand);
        let first = self.parse_expression()?;

        let (key, value, by_ref) = if self.eat(&TokenKind::DoubleArrow) {
            // Key was marked as by-ref - this is an error
            if by_ref {
                return Err(ParseError {
                    message: "Key element cannot be a reference".to_string(),
                    span,
                });
            }
            let by_ref_val = self.eat(&TokenKind::Ampersand);
            let value = self.parse_expression()?;
            (Some(first), value, by_ref_val)
        } else {
            (None, first, by_ref)
        };

        self.expect(&TokenKind::CloseParen)?;
        let body = if matches!(self.peek(), TokenKind::OpenBrace) {
            self.parse_block()?
        } else if self.eat(&TokenKind::Colon) {
            // Alternative syntax: foreach (): ... endforeach;
            let mut stmts = Vec::new();
            while !matches!(self.peek(), TokenKind::EndForeach | TokenKind::Eof) {
                stmts.push(self.parse_statement()?);
            }
            self.expect(&TokenKind::EndForeach)?;
            self.expect_semicolon()?;
            stmts
        } else {
            vec![self.parse_statement()?]
        };

        Ok(Statement {
            kind: StmtKind::Foreach {
                expr,
                key,
                value,
                by_ref,
                body,
            },
            span,
        })
    }

    fn parse_switch(&mut self) -> ParseResult<Statement> {
        let span = self.span();
        self.advance(); // switch
        self.expect(&TokenKind::OpenParen)?;
        let expr = self.parse_expression()?;
        self.expect(&TokenKind::CloseParen)?;

        // Check for alternative syntax: switch(): ... endswitch;
        let use_alt_syntax = if matches!(self.peek(), TokenKind::Colon) {
            self.advance();
            true
        } else {
            self.expect(&TokenKind::OpenBrace)?;
            false
        };

        let end_token = if use_alt_syntax {
            TokenKind::EndSwitch
        } else {
            TokenKind::CloseBrace
        };

        let mut cases = Vec::new();
        while std::mem::discriminant(self.peek()) != std::mem::discriminant(&end_token)
            && !matches!(self.peek(), TokenKind::Eof)
        {
            let value = if self.eat(&TokenKind::Case) {
                let v = self.parse_expression()?;
                self.expect(&TokenKind::Colon)?;
                Some(v)
            } else if self.eat(&TokenKind::Default) {
                self.expect(&TokenKind::Colon)?;
                None
            } else {
                return Err(ParseError {
                    message: "expected 'case' or 'default'".into(),
                    span: self.span(),
                });
            };

            let mut body = Vec::new();
            while !matches!(
                self.peek(),
                TokenKind::Case | TokenKind::Default | TokenKind::Eof
            ) && std::mem::discriminant(self.peek()) != std::mem::discriminant(&end_token)
            {
                body.push(self.parse_statement()?);
            }
            cases.push(SwitchCase { value, body });
        }

        if use_alt_syntax {
            self.expect(&TokenKind::EndSwitch)?;
            self.expect_semicolon()?;
        } else {
            self.expect(&TokenKind::CloseBrace)?;
        }
        Ok(Statement {
            kind: StmtKind::Switch { expr, cases },
            span,
        })
    }

    fn parse_function_decl_with_attrs(&mut self, attributes: Vec<Attribute>) -> ParseResult<Statement> {
        let span = self.span();
        self.advance(); // function
        // Optional & for return-by-reference
        self.eat(&TokenKind::Ampersand);
        let name = match self.peek().clone() {
            TokenKind::Identifier(name) => {
                self.advance();
                name
            }
            _ if self.is_semi_reserved_keyword() => {
                let kw = self.keyword_to_identifier();
                self.advance();
                kw
            }
            _ => {
                return Err(ParseError {
                    message: "expected function name".into(),
                    span: self.span(),
                });
            }
        };

        self.expect(&TokenKind::OpenParen)?;
        let params = self.parse_params()?;
        self.expect(&TokenKind::CloseParen)?;

        let return_type = if self.eat(&TokenKind::Colon) {
            Some(self.parse_type_hint()?)
        } else {
            None
        };

        let body = self.parse_block()?;

        Ok(Statement {
            kind: StmtKind::FunctionDecl {
                name,
                params,
                return_type,
                body,
                is_static: false,
                attributes,
            },
            span,
        })
    }

    fn parse_params(&mut self) -> ParseResult<Vec<Param>> {
        let mut params = Vec::new();
        if matches!(self.peek(), TokenKind::CloseParen) {
            return Ok(params);
        }

        loop {
            let param_attributes = self.parse_attributes()?;
            let mut visibility = None;
            let mut set_visibility = None;
            let mut readonly = false;
            let mut is_final = false;

            // Constructor promotion modifiers (possibly with asymmetric set visibility)
            loop {
                match self.peek() {
                    TokenKind::Public | TokenKind::Protected | TokenKind::Private => {
                        let vis = match self.peek() {
                            TokenKind::Protected => Visibility::Protected,
                            TokenKind::Private => Visibility::Private,
                            _ => Visibility::Public,
                        };
                        self.advance();
                        // Check for asymmetric visibility: private(set), protected(set), public(set)
                        if matches!(self.peek(), TokenKind::OpenParen) && self.lookahead_is_set_modifier() {
                            self.advance(); // consume (
                            self.advance(); // consume 'set'
                            self.advance(); // consume )
                            set_visibility = Some(vis);
                        } else {
                            visibility = Some(vis);
                        }
                    }
                    TokenKind::Final => {
                        is_final = true;
                        self.advance();
                    }
                    TokenKind::Readonly => {
                        readonly = true;
                        self.advance();
                    }
                    _ => break,
                }
            }

            let type_hint = if !matches!(
                self.peek(),
                TokenKind::Variable(_) | TokenKind::Ampersand | TokenKind::Ellipsis
            ) {
                Some(self.parse_type_hint()?)
            } else {
                None
            };

            let by_ref = self.eat(&TokenKind::Ampersand);
            let variadic = self.eat(&TokenKind::Ellipsis);

            let name = match self.peek().clone() {
                TokenKind::Variable(name) => {
                    self.advance();
                    name
                }
                _ => {
                    return Err(ParseError {
                        message: "expected parameter name".into(),
                        span: self.span(),
                    });
                }
            };

            let default = if self.eat(&TokenKind::Assign) {
                Some(self.parse_expression()?)
            } else {
                None
            };

            // Parse property hooks for promoted properties: public int $x { get => ...; set => ...; }
            // Hooks can appear even without explicit visibility modifier (implicit promotion in PHP 8.4)
            let (param_get_hook, param_set_hook) = if matches!(self.peek(), TokenKind::OpenBrace) {
                self.parse_property_hooks(&name)?
            } else {
                (None, None)
            };

            params.push(Param {
                name,
                type_hint,
                default,
                by_ref,
                variadic,
                visibility,
                set_visibility,
                readonly,
                is_final,
                attributes: param_attributes,
                get_hook: param_get_hook,
                set_hook: param_set_hook,
            });

            if !self.eat(&TokenKind::Comma) {
                break;
            }
            // Allow trailing comma
            if matches!(self.peek(), TokenKind::CloseParen) {
                break;
            }
        }

        Ok(params)
    }

    fn parse_type_hint(&mut self) -> ParseResult<TypeHint> {
        if self.eat(&TokenKind::QuestionMark) {
            let inner = self.parse_simple_type()?;
            return Ok(TypeHint::Nullable(Box::new(inner)));
        }

        let first = self.parse_type_atom()?;

        // Check for union or intersection
        if matches!(self.peek(), TokenKind::Pipe) {
            let mut types = vec![first];
            while self.eat(&TokenKind::Pipe) {
                types.push(self.parse_type_atom()?);
            }
            return Ok(TypeHint::Union(types));
        }

        if matches!(self.peek(), TokenKind::Ampersand) {
            // Check if this is an intersection type or a by-ref parameter marker.
            // If the token after & is a Variable or Ellipsis, it's by-ref, not intersection.
            if let Some(after_amp) = self.tokens.get(self.pos + 1) {
                if matches!(after_amp.kind, TokenKind::Variable(_) | TokenKind::Ellipsis) {
                    return Ok(first);
                }
            }
            let mut types = vec![first];
            while self.eat(&TokenKind::Ampersand) {
                // Check again if the next token after & is a variable (by-ref marker for next param)
                if matches!(self.peek(), TokenKind::Variable(_) | TokenKind::Ellipsis) {
                    // We already consumed the &, but it was by-ref, not intersection.
                    // We need to put it back - but we can't easily. Instead, just break.
                    // Actually this case shouldn't happen since we checked above.
                    break;
                }
                types.push(self.parse_simple_type()?);
            }
            if types.len() > 1 {
                // Validate: scalar types cannot be part of an intersection
                for t in &types {
                    if let Some(err_name) = Self::check_invalid_intersection_type(t) {
                        return Err(ParseError {
                            message: format!("Type {} cannot be part of an intersection type", err_name),
                            span: self.span(),
                        });
                    }
                }
                return Ok(TypeHint::Intersection(types));
            }
            return Ok(types.into_iter().next().unwrap());
        }

        Ok(first)
    }

    /// Parse a type atom: either a simple type or a parenthesized intersection type (for DNF types).
    fn parse_type_atom(&mut self) -> ParseResult<TypeHint> {
        if self.eat(&TokenKind::OpenParen) {
            // Parenthesized intersection type: (A&B)
            let first = self.parse_simple_type()?;
            let mut types = vec![first];
            while self.eat(&TokenKind::Ampersand) {
                types.push(self.parse_simple_type()?);
            }
            self.expect(&TokenKind::CloseParen)?;
            if types.len() == 1 {
                return Ok(types.into_iter().next().unwrap());
            }
            // Validate: scalar types cannot be part of an intersection
            for t in &types {
                if let Some(err_name) = Self::check_invalid_intersection_type(t) {
                    return Err(ParseError {
                        message: format!("Type {} cannot be part of an intersection type", err_name),
                        span: self.span(),
                    });
                }
            }
            return Ok(TypeHint::Intersection(types));
        }
        self.parse_simple_type()
    }

    /// Parse a class/interface/trait name reference that may be qualified.
    /// Handles: Foo, Foo\Bar, \Foo\Bar (fully qualified, prefixed with \)
    fn parse_class_name_ref(&mut self) -> ParseResult<Vec<u8>> {
        let has_leading_backslash = self.eat(&TokenKind::Backslash);
        let mut name = if has_leading_backslash {
            vec![b'\\']
        } else {
            Vec::new()
        };
        match self.peek().clone() {
            TokenKind::Identifier(part) => {
                self.advance();
                name.extend_from_slice(&part);
            }
            _ if self.is_semi_reserved_keyword() => {
                name.extend_from_slice(&self.keyword_to_identifier());
                self.advance();
            }
            _ => {
                return Err(ParseError {
                    message: "expected class name".into(),
                    span: self.span(),
                });
            }
        }
        while self.eat(&TokenKind::Backslash) {
            match self.peek().clone() {
                TokenKind::Identifier(part) => {
                    self.advance();
                    name.push(b'\\');
                    name.extend_from_slice(&part);
                }
                _ if self.is_semi_reserved_keyword() => {
                    name.push(b'\\');
                    name.extend_from_slice(&self.keyword_to_identifier());
                    self.advance();
                }
                _ => break,
            }
        }
        Ok(name)
    }

    /// Check if a type hint is a scalar/builtin type that cannot be part of an intersection type
    fn check_invalid_intersection_type(hint: &TypeHint) -> Option<String> {
        match hint {
            TypeHint::Simple(name) => {
                let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
                match lower.as_slice() {
                    b"int" | b"float" | b"string" | b"bool" | b"array" | b"null"
                    | b"void" | b"never" | b"object" | b"mixed" | b"callable"
                    | b"false" | b"true" | b"static" | b"iterable" => {
                        let display = match lower.as_slice() {
                            b"iterable" => "Traversable|array".to_string(),
                            _ => String::from_utf8_lossy(name).to_string(),
                        };
                        Some(display)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn parse_simple_type(&mut self) -> ParseResult<TypeHint> {
        // Handle leading backslash for fully qualified names
        if self.eat(&TokenKind::Backslash) {
            // Fully qualified name - prefix with \ to mark as absolute
            let mut name = vec![b'\\'];
            loop {
                match self.peek().clone() {
                    TokenKind::Identifier(part) => {
                        self.advance();
                        if name.len() > 1 {
                            name.push(b'\\');
                        }
                        name.extend_from_slice(&part);
                    }
                    _ => break,
                }
                if !self.eat(&TokenKind::Backslash) {
                    break;
                }
            }
            return Ok(TypeHint::Simple(name));
        }
        match self.peek().clone() {
            TokenKind::Identifier(name) => {
                self.advance();
                // Check for qualified name continuation
                let mut full_name = name;
                while self.eat(&TokenKind::Backslash) {
                    if let TokenKind::Identifier(part) = self.peek().clone() {
                        self.advance();
                        full_name.push(b'\\');
                        full_name.extend_from_slice(&part);
                    } else {
                        break;
                    }
                }
                Ok(TypeHint::Simple(full_name))
            }
            TokenKind::Array => {
                self.advance();
                Ok(TypeHint::Simple(b"array".to_vec()))
            }
            TokenKind::Callable => {
                self.advance();
                Ok(TypeHint::Simple(b"callable".to_vec()))
            }
            TokenKind::Null => {
                self.advance();
                Ok(TypeHint::Simple(b"null".to_vec()))
            }
            TokenKind::True => {
                self.advance();
                Ok(TypeHint::Simple(b"true".to_vec()))
            }
            TokenKind::False => {
                self.advance();
                Ok(TypeHint::Simple(b"false".to_vec()))
            }
            TokenKind::Static => {
                self.advance();
                Ok(TypeHint::Simple(b"static".to_vec()))
            }
            TokenKind::Var => {
                // PHP 4 compat: var is sometimes used in old code
                self.advance();
                Ok(TypeHint::Simple(b"var".to_vec()))
            }
            _ => Err(ParseError {
                message: format!("expected type name, found {:?}", self.peek()),
                span: self.span(),
            }),
        }
    }

    fn parse_class_decl_with_attrs(&mut self, attributes: Vec<Attribute>) -> ParseResult<Statement> {
        let span = self.span();
        let mut modifiers = ClassModifiers::default();

        loop {
            match self.peek() {
                TokenKind::Abstract => {
                    if modifiers.is_abstract {
                        return Err(ParseError {
                            message: "Multiple abstract modifiers are not allowed".into(),
                            span: self.span(),
                        });
                    }
                    modifiers.is_abstract = true;
                    self.advance();
                }
                TokenKind::Final => {
                    if modifiers.is_final {
                        return Err(ParseError {
                            message: "Multiple final modifiers are not allowed".into(),
                            span: self.span(),
                        });
                    }
                    modifiers.is_final = true;
                    self.advance();
                }
                TokenKind::Readonly => {
                    if modifiers.is_readonly {
                        return Err(ParseError {
                            message: "Multiple readonly modifiers are not allowed".into(),
                            span: self.span(),
                        });
                    }
                    modifiers.is_readonly = true;
                    self.advance();
                }
                _ => break,
            }
        }

        // Check for invalid combinations
        if modifiers.is_abstract && modifiers.is_final {
            return Err(ParseError {
                message: "Cannot use the final modifier on an abstract class".into(),
                span: span.clone(),
            });
        }

        // Accept class, interface, trait, or enum
        match self.peek() {
            TokenKind::Interface => {
                if modifiers.is_readonly {
                    return Err(ParseError { message: "syntax error, unexpected token \"interface\", expecting \"abstract\" or \"final\" or \"readonly\" or \"class\"".into(), span: self.span() });
                }
                modifiers.is_interface = true;
                self.advance();
            }
            TokenKind::Trait => {
                if modifiers.is_readonly {
                    return Err(ParseError { message: "syntax error, unexpected token \"trait\", expecting \"abstract\" or \"final\" or \"readonly\" or \"class\"".into(), span: self.span() });
                }
                modifiers.is_trait = true;
                self.advance();
            }
            TokenKind::Enum => {
                if modifiers.is_readonly {
                    return Err(ParseError { message: "syntax error, unexpected token \"enum\", expecting \"abstract\" or \"final\" or \"readonly\" or \"class\"".into(), span: self.span() });
                }
                modifiers.is_enum = true;
                self.advance();
            }
            TokenKind::Class => {
                self.advance();
            }
            _ => {
                return Err(ParseError {
                    message: "expected class, interface, trait, or enum keyword".into(),
                    span: self.span(),
                });
            }
        }
        let name = match self.peek().clone() {
            TokenKind::Identifier(name) => {
                self.advance();
                name
            }
            _ if self.is_semi_reserved_keyword() => {
                let kw = self.keyword_to_identifier();
                self.advance();
                kw
            }
            _ => {
                return Err(ParseError {
                    message: "expected class name".into(),
                    span: self.span(),
                });
            }
        };

        // For enums: capture `: type` backing type declaration
        let mut enum_backing_type: Option<Vec<u8>> = None;
        if self.eat(&TokenKind::Colon) {
            // Capture the backing type name (e.g., string, int)
            let backing = match self.peek() {
                TokenKind::Identifier(name) => {
                    let n = name.clone();
                    self.advance();
                    n
                }
                _ if self.is_semi_reserved_keyword() => {
                    // Handle keywords used as type names
                    let name = match self.peek() {
                        TokenKind::Array => b"array".to_vec(),
                        _ => b"mixed".to_vec(),
                    };
                    self.advance();
                    name
                }
                _ => {
                    self.advance(); // skip unknown
                    b"mixed".to_vec()
                }
            };
            if modifiers.is_enum {
                enum_backing_type = Some(backing);
            }
        }

        let extends = if matches!(self.peek(), TokenKind::Extends) {
            if modifiers.is_trait {
                return Err(ParseError {
                    message: "syntax error, unexpected token \"extends\", expecting \"{\"".into(),
                    span: self.span(),
                });
            }
            self.advance(); // eat extends
            let first = self.parse_class_name_ref()?;
            Some(first)
        } else {
            None
        };

        // For interfaces: "extends A, B, C" - additional names after comma go into implements
        let mut implements = Vec::new();
        if extends.is_some() && modifiers.is_interface {
            while self.eat(&TokenKind::Comma) {
                implements.push(self.parse_class_name_ref()?);
            }
        }
        if self.eat(&TokenKind::Implements) {
            loop {
                implements.push(self.parse_class_name_ref()?);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }

        self.expect(&TokenKind::OpenBrace)?;
        let mut body = Vec::new();
        while !matches!(self.peek(), TokenKind::CloseBrace | TokenKind::Eof) {
            body.extend(self.parse_class_members()?);
        }
        self.expect(&TokenKind::CloseBrace)?;

        Ok(Statement {
            kind: StmtKind::ClassDecl {
                name,
                modifiers,
                extends,
                implements,
                body,
                enum_backing_type,
                attributes,
            },
            span,
        })
    }

    fn parse_class_body(&mut self) -> ParseResult<Vec<ClassMember>> {
        self.expect(&TokenKind::OpenBrace)?;
        let mut body = Vec::new();
        while !matches!(self.peek(), TokenKind::CloseBrace | TokenKind::Eof) {
            body.extend(self.parse_class_members()?);
        }
        self.expect(&TokenKind::CloseBrace)?;
        Ok(body)
    }

    fn parse_class_members(&mut self) -> ParseResult<Vec<ClassMember>> {
        let member = self.parse_class_member()?;
        let mut members = vec![member];
        // Drain any extra constants from comma-separated declarations
        members.append(&mut self.pending_extra_constants);
        // Drain any extra properties from comma-separated declarations
        members.append(&mut self.pending_extra_properties);
        Ok(members)
    }

    fn parse_class_member(&mut self) -> ParseResult<ClassMember> {
        let member_attributes = self.parse_attributes()?;
        let mut visibility = Visibility::Public;
        let mut is_static = false;
        let mut is_abstract = false;
        let mut is_final = false;
        let mut is_readonly = false;
        let mut has_visibility = false;
        let mut set_visibility: Option<Visibility> = None;
        let mut has_set_visibility = false;

        // Parse modifiers
        loop {
            match self.peek() {
                TokenKind::Public | TokenKind::Protected | TokenKind::Private => {
                    let vis = match self.peek() {
                        TokenKind::Protected => Visibility::Protected,
                        TokenKind::Private => Visibility::Private,
                        _ => Visibility::Public,
                    };
                    self.advance();
                    // Check for asymmetric visibility: private(set), protected(set), public(set)
                    if matches!(self.peek(), TokenKind::OpenParen) {
                        // Peek ahead to see if it's `(set)`
                        if self.lookahead_is_set_modifier() {
                            if has_set_visibility {
                                return Err(ParseError {
                                    message: "Multiple access type modifiers are not allowed".into(),
                                    span: self.span(),
                                });
                            }
                            has_set_visibility = true;
                            self.advance(); // consume (
                            self.advance(); // consume 'set'
                            self.advance(); // consume )
                            set_visibility = Some(vis);
                            continue;
                        }
                    }
                    // Normal (read) visibility
                    if has_visibility {
                        return Err(ParseError {
                            message: "Multiple access type modifiers are not allowed".into(),
                            span: self.span(),
                        });
                    }
                    has_visibility = true;
                    visibility = vis;
                }
                TokenKind::Static => {
                    if is_static {
                        return Err(ParseError {
                            message: "Multiple static modifiers are not allowed".into(),
                            span: self.span(),
                        });
                    }
                    is_static = true;
                    self.advance();
                }
                TokenKind::Abstract => {
                    if is_abstract {
                        return Err(ParseError {
                            message: "Multiple abstract modifiers are not allowed".into(),
                            span: self.span(),
                        });
                    }
                    is_abstract = true;
                    self.advance();
                }
                TokenKind::Final => {
                    if is_final {
                        return Err(ParseError {
                            message: "Multiple final modifiers are not allowed".into(),
                            span: self.span(),
                        });
                    }
                    is_final = true;
                    self.advance();
                }
                TokenKind::Readonly => {
                    if is_readonly {
                        return Err(ParseError {
                            message: "Multiple readonly modifiers are not allowed".into(),
                            span: self.span(),
                        });
                    }
                    is_readonly = true;
                    self.advance();
                }
                TokenKind::Var => {
                    // PHP 4 compat: var = public
                    visibility = Visibility::Public;
                    self.advance();
                }
                _ => break,
            }
        }

        match self.peek() {
            TokenKind::Case => {
                // enum case
                self.advance();
                let name = match self.peek().clone() {
                    TokenKind::Identifier(name) => {
                        self.advance();
                        name
                    }
                    _ if self.is_semi_reserved_keyword() => {
                        let kw = self.keyword_to_identifier();
                        self.advance();
                        kw
                    }
                    _ => {
                        return Err(ParseError {
                            message: "expected enum case name".into(),
                            span: self.span(),
                        });
                    }
                };
                let value = if self.eat(&TokenKind::Assign) {
                    Some(self.parse_expression()?)
                } else {
                    None
                };
                self.expect(&TokenKind::Semicolon)?;
                return Ok(ClassMember::EnumCase { name, value, attributes: member_attributes });
            }
            TokenKind::Use => {
                // trait use
                self.advance();
                let mut traits = Vec::new();
                loop {
                    if matches!(self.peek(), TokenKind::Identifier(_) | TokenKind::Backslash) {
                        traits.push(self.parse_class_name_ref()?);
                    } else {
                        break;
                    }
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                // Handle trait conflict resolution block
                let mut adaptations = Vec::new();
                if matches!(self.peek(), TokenKind::OpenBrace) {
                    self.advance(); // consume {
                    while !matches!(self.peek(), TokenKind::CloseBrace | TokenKind::Eof) {
                        // Parse: TraitName::method insteadof OtherTrait;
                        //    or: TraitName::method as [visibility] [newName];
                        //    or: method as [visibility] [newName];
                        let first_name = match self.peek().clone() {
                            TokenKind::Identifier(name) => {
                                self.advance();
                                name
                            }
                            _ => {
                                self.advance(); // skip unexpected
                                continue;
                            }
                        };

                        if self.eat(&TokenKind::DoubleColon) {
                            // TraitName::method
                            let method_name = match self.peek().clone() {
                                TokenKind::Identifier(name) => {
                                    self.advance();
                                    name
                                }
                                _ => {
                                    self.advance();
                                    continue;
                                }
                            };

                            if matches!(self.peek(), TokenKind::Insteadof) {
                                self.advance(); // consume insteadof
                                let mut instead_of = Vec::new();
                                loop {
                                    let iname = self.parse_class_name_ref()?;
                                    instead_of.push(iname);
                                    if !self.eat(&TokenKind::Comma) {
                                        break;
                                    }
                                }
                                adaptations.push(TraitAdaptation::Precedence {
                                    trait_name: first_name,
                                    method: method_name,
                                    instead_of,
                                });
                            } else if matches!(self.peek(), TokenKind::As) {
                                self.advance(); // consume as
                                let mut new_visibility = None;
                                let mut new_name = None;
                                match self.peek() {
                                    TokenKind::Public => {
                                        new_visibility = Some(Visibility::Public);
                                        self.advance();
                                    }
                                    TokenKind::Protected => {
                                        new_visibility = Some(Visibility::Protected);
                                        self.advance();
                                    }
                                    TokenKind::Private => {
                                        new_visibility = Some(Visibility::Private);
                                        self.advance();
                                    }
                                    _ => {}
                                }
                                if let TokenKind::Identifier(name) = self.peek().clone() {
                                    new_name = Some(name);
                                    self.advance();
                                }
                                adaptations.push(TraitAdaptation::Alias {
                                    trait_name: Some(first_name),
                                    method: method_name,
                                    new_name,
                                    new_visibility,
                                });
                            }
                        } else if matches!(self.peek(), TokenKind::As) {
                            // method as [visibility] [newName]
                            self.advance(); // consume as
                            let mut new_visibility = None;
                            let mut new_name = None;
                            match self.peek() {
                                TokenKind::Public => {
                                    new_visibility = Some(Visibility::Public);
                                    self.advance();
                                }
                                TokenKind::Protected => {
                                    new_visibility = Some(Visibility::Protected);
                                    self.advance();
                                }
                                TokenKind::Private => {
                                    new_visibility = Some(Visibility::Private);
                                    self.advance();
                                }
                                _ => {}
                            }
                            if let TokenKind::Identifier(name) = self.peek().clone() {
                                new_name = Some(name);
                                self.advance();
                            }
                            adaptations.push(TraitAdaptation::Alias {
                                trait_name: None,
                                method: first_name,
                                new_name,
                                new_visibility,
                            });
                        }
                        self.eat(&TokenKind::Semicolon);
                    }
                    self.eat(&TokenKind::CloseBrace);
                } else {
                    self.expect_semicolon()?;
                }
                Ok(ClassMember::TraitUse {
                    traits,
                    adaptations,
                })
            }
            TokenKind::Function => {
                if is_readonly {
                    return Err(ParseError { message: "Cannot use the readonly modifier on a method".into(), span: self.span() });
                }
                let method_line = self.span().line;
                self.advance();
                // Optional & for return-by-reference
                self.eat(&TokenKind::Ampersand);
                let name = match self.peek().clone() {
                    TokenKind::Identifier(name) => {
                        self.advance();
                        name
                    }
                    // Allow keywords as method names
                    _ if self.is_semi_reserved_keyword() => {
                        let kw = self.keyword_to_identifier();
                        self.advance();
                        kw
                    }
                    _ => {
                        return Err(ParseError {
                            message: "expected method name".into(),
                            span: self.span(),
                        });
                    }
                };

                self.expect(&TokenKind::OpenParen)?;
                let params = self.parse_params()?;
                self.expect(&TokenKind::CloseParen)?;

                let return_type = if self.eat(&TokenKind::Colon) {
                    Some(self.parse_type_hint()?)
                } else {
                    None
                };

                let body = if is_abstract {
                    if matches!(self.peek(), TokenKind::Semicolon) {
                        self.expect_semicolon()?;
                        None
                    } else if matches!(self.peek(), TokenKind::OpenBrace) {
                        // Abstract method with body - parse it but it will be an error
                        Some(self.parse_block()?)
                    } else {
                        self.expect_semicolon()?;
                        None
                    }
                } else if matches!(self.peek(), TokenKind::Semicolon) {
                    self.expect_semicolon()?;
                    None
                } else {
                    Some(self.parse_block()?)
                };

                Ok(ClassMember::Method {
                    name,
                    params,
                    return_type,
                    body,
                    visibility,
                    is_static,
                    is_abstract,
                    is_final,
                    line: method_line,
                    attributes: member_attributes,
                })
            }
            TokenKind::Const => {
                if is_readonly {
                    return Err(ParseError { message: "Cannot use the readonly modifier on a class constant".into(), span: self.span() });
                }
                self.advance();
                // Try to parse an identifier - it might be a type hint or the constant name
                let first_name = match self.peek().clone() {
                    TokenKind::Identifier(name) => {
                        self.advance();
                        name
                    }
                    _ if self.is_semi_reserved_keyword() => {
                        let kw = self.keyword_to_identifier();
                        self.advance();
                        kw
                    }
                    TokenKind::QuestionMark => {
                        // Nullable type hint: ?int, ?string, etc.
                        self.advance();
                        // Skip the type name
                        match self.peek() {
                            TokenKind::Identifier(_) => { self.advance(); }
                            _ if self.is_semi_reserved_keyword() => { self.advance(); }
                            _ => { self.advance(); }
                        }
                        // Now parse the actual constant name
                        let name = match self.peek().clone() {
                            TokenKind::Identifier(name) => { self.advance(); name }
                            _ if self.is_semi_reserved_keyword() => { let kw = self.keyword_to_identifier(); self.advance(); kw }
                            _ => return Err(ParseError { message: "expected constant name".into(), span: self.span() }),
                        };
                        self.expect(&TokenKind::Assign)?;
                        let value = self.parse_expression()?;
                        // Handle comma-separated typed constants
                        self.pending_extra_constants.clear();
                        while self.eat(&TokenKind::Comma) {
                            let extra_name = match self.peek().clone() {
                                TokenKind::Identifier(n) => { self.advance(); n }
                                _ if self.is_semi_reserved_keyword() => { let kw = self.keyword_to_identifier(); self.advance(); kw }
                                _ => break,
                            };
                            self.expect(&TokenKind::Assign)?;
                            let extra_value = self.parse_expression()?;
                            self.pending_extra_constants.push(ClassMember::ClassConstant {
                                name: extra_name,
                                value: extra_value,
                                visibility,
                                is_final,
                                    attributes: member_attributes.clone(),
                            });
                        }
                        self.expect_semicolon()?;
                        return Ok(ClassMember::ClassConstant { name, value, visibility, is_final, attributes: member_attributes.clone() });
                    }
                    TokenKind::OpenParen => {
                        // DNF type hint: (A&B)|null CONST = value
                        // Skip the entire parenthesized type expression
                        self.advance(); // skip (
                        let mut depth = 1u32;
                        while depth > 0 {
                            match self.peek() {
                                TokenKind::OpenParen => { depth += 1; self.advance(); }
                                TokenKind::CloseParen => { depth -= 1; self.advance(); }
                                TokenKind::Eof => break,
                                _ => { self.advance(); }
                            }
                        }
                        // There may be a union | after the parenthesized group
                        while matches!(self.peek(), TokenKind::Pipe | TokenKind::Ampersand) {
                            self.advance(); // skip | or &
                            if matches!(self.peek(), TokenKind::OpenParen) {
                                self.advance();
                                let mut d = 1u32;
                                while d > 0 {
                                    match self.peek() {
                                        TokenKind::OpenParen => { d += 1; self.advance(); }
                                        TokenKind::CloseParen => { d -= 1; self.advance(); }
                                        TokenKind::Eof => break,
                                        _ => { self.advance(); }
                                    }
                                }
                            } else {
                                match self.peek() {
                                    TokenKind::Identifier(_) => { self.advance(); }
                                    _ if self.is_semi_reserved_keyword() => { self.advance(); }
                                    _ => { self.advance(); }
                                }
                            }
                        }
                        // Now parse the actual constant name
                        let name = match self.peek().clone() {
                            TokenKind::Identifier(name) => { self.advance(); name }
                            _ if self.is_semi_reserved_keyword() => { let kw = self.keyword_to_identifier(); self.advance(); kw }
                            _ => return Err(ParseError { message: "expected constant name".into(), span: self.span() }),
                        };
                        self.expect(&TokenKind::Assign)?;
                        let value = self.parse_expression()?;
                        self.pending_extra_constants.clear();
                        while self.eat(&TokenKind::Comma) {
                            let extra_name = match self.peek().clone() {
                                TokenKind::Identifier(n) => { self.advance(); n }
                                _ if self.is_semi_reserved_keyword() => { let kw = self.keyword_to_identifier(); self.advance(); kw }
                                _ => break,
                            };
                            self.expect(&TokenKind::Assign)?;
                            let extra_value = self.parse_expression()?;
                            self.pending_extra_constants.push(ClassMember::ClassConstant {
                                name: extra_name,
                                value: extra_value,
                                visibility,
                                is_final,
                                    attributes: member_attributes.clone(),
                            });
                        }
                        self.expect_semicolon()?;
                        return Ok(ClassMember::ClassConstant { name, value, visibility, is_final, attributes: member_attributes.clone() });
                    }
                    _ => {
                        return Err(ParseError {
                            message: "expected constant name".into(),
                            span: self.span(),
                        });
                    }
                };
                // Check if the next token is `=` (meaning first_name IS the constant name)
                // or if it's an identifier (meaning first_name was a type hint)
                let name = if matches!(self.peek(), TokenKind::Assign) {
                    first_name
                } else if matches!(self.peek(), TokenKind::Pipe | TokenKind::Ampersand) {
                    // Union/intersection type hint: int|string, Foo&Bar
                    // Skip over the type hint tokens
                    while matches!(self.peek(), TokenKind::Pipe | TokenKind::Ampersand) {
                        self.advance(); // skip | or &
                        match self.peek() {
                            TokenKind::Identifier(_) => { self.advance(); }
                            _ if self.is_semi_reserved_keyword() => { self.advance(); }
                            _ => { self.advance(); }
                        }
                    }
                    // Now parse the actual constant name
                    match self.peek().clone() {
                        TokenKind::Identifier(name) => { self.advance(); name }
                        _ if self.is_semi_reserved_keyword() => { let kw = self.keyword_to_identifier(); self.advance(); kw }
                        _ => return Err(ParseError { message: "expected constant name".into(), span: self.span() }),
                    }
                } else {
                    // first_name was a type hint, parse actual constant name
                    match self.peek().clone() {
                        TokenKind::Identifier(name) => { self.advance(); name }
                        _ if self.is_semi_reserved_keyword() => { let kw = self.keyword_to_identifier(); self.advance(); kw }
                        _ => return Err(ParseError { message: "expected constant name after type hint".into(), span: self.span() }),
                    }
                };
                self.expect(&TokenKind::Assign)?;
                let value = self.parse_expression()?;
                // Handle comma-separated constants: const A = 1, B = 2;
                // We only return the first one here; additional ones are handled by parse_class_members
                self.pending_extra_constants.clear();
                while self.eat(&TokenKind::Comma) {
                    let extra_name = match self.peek().clone() {
                        TokenKind::Identifier(n) => { self.advance(); n }
                        _ if self.is_semi_reserved_keyword() => { let kw = self.keyword_to_identifier(); self.advance(); kw }
                        _ => break,
                    };
                    self.expect(&TokenKind::Assign)?;
                    let extra_value = self.parse_expression()?;
                    self.pending_extra_constants.push(ClassMember::ClassConstant {
                        name: extra_name,
                        value: extra_value,
                        visibility,
                        is_final,
                                    attributes: member_attributes.clone(),
                    });
                }
                self.expect_semicolon()?;
                Ok(ClassMember::ClassConstant {
                    name,
                    value,
                    visibility,
                    is_final,
                    attributes: member_attributes,
                })
            }
            TokenKind::Variable(_) => {
                // Property (possibly with type hint already consumed as a modifier)
                let name = match self.peek().clone() {
                    TokenKind::Variable(name) => {
                        self.advance();
                        name
                    }
                    _ => unreachable!(),
                };
                let default = if self.eat(&TokenKind::Assign) {
                    Some(self.parse_expression()?)
                } else {
                    None
                };
                // Check for property hooks { get { ... } set(...) { ... } }
                let (get_hook, set_hook) = if matches!(self.peek(), TokenKind::OpenBrace) {
                    self.parse_property_hooks(&name)?
                } else {
                    // Parse comma-separated additional properties
                    self.pending_extra_properties.clear();
                    while self.eat(&TokenKind::Comma) {
                        if let TokenKind::Variable(extra_name) = self.peek().clone() {
                            self.advance();
                            let extra_default = if self.eat(&TokenKind::Assign) {
                                Some(self.parse_expression()?)
                            } else {
                                None
                            };
                            self.pending_extra_properties.push(ClassMember::Property {
                                name: extra_name,
                                type_hint: None,
                                default: extra_default,
                                visibility,
                                set_visibility,
                                is_static,
                                is_readonly,
                                get_hook: None,
                                set_hook: None,
                                attributes: member_attributes.clone(),
                            });
                        } else {
                            break;
                        }
                    }
                    self.expect_semicolon()?;
                    (None, None)
                };
                // Abstract properties must have hooks
                if is_abstract && get_hook.is_none() && set_hook.is_none() {
                    return Err(ParseError {
                        message: "Only hooked properties may be declared abstract".into(),
                        span: self.span(),
                    });
                }
                Ok(ClassMember::Property {
                    name,
                    type_hint: None,
                    default,
                    visibility,
                    set_visibility,
                    is_static,
                    is_readonly,
                    get_hook,
                    set_hook,
                    attributes: member_attributes.clone(),
                })
            }
            _ => {
                // Might be a typed property: type $name
                let type_hint = self.parse_type_hint()?;
                let name = match self.peek().clone() {
                    TokenKind::Variable(name) => {
                        self.advance();
                        name
                    }
                    _ => {
                        return Err(ParseError {
                            message: "expected property name".into(),
                            span: self.span(),
                        });
                    }
                };
                let default = if self.eat(&TokenKind::Assign) {
                    Some(self.parse_expression()?)
                } else {
                    None
                };
                // Check for property hooks { get { ... } set(...) { ... } }
                let (get_hook, set_hook) = if matches!(self.peek(), TokenKind::OpenBrace) {
                    self.parse_property_hooks(&name)?
                } else {
                    // Parse comma-separated additional properties (they share the same type)
                    while self.eat(&TokenKind::Comma) {
                        if let TokenKind::Variable(extra_name) = self.peek().clone() {
                            self.advance();
                            let extra_default = if self.eat(&TokenKind::Assign) {
                                Some(self.parse_expression()?)
                            } else {
                                None
                            };
                            self.pending_extra_properties.push(ClassMember::Property {
                                name: extra_name,
                                type_hint: Some(type_hint.clone()),
                                default: extra_default,
                                visibility,
                                set_visibility,
                                is_static,
                                is_readonly,
                                get_hook: None,
                                set_hook: None,
                                attributes: member_attributes.clone(),
                            });
                        } else {
                            break;
                        }
                    }
                    self.expect_semicolon()?;
                    (None, None)
                };
                // Abstract properties must have hooks
                if is_abstract && get_hook.is_none() && set_hook.is_none() {
                    return Err(ParseError {
                        message: "Only hooked properties may be declared abstract".into(),
                        span: self.span(),
                    });
                }
                Ok(ClassMember::Property {
                    name,
                    type_hint: Some(type_hint),
                    default,
                    visibility,
                    set_visibility,
                    is_static,
                    is_readonly,
                    get_hook,
                    set_hook,
                    attributes: member_attributes,
                })
            }
        }
    }

    /// Parse property hooks: { get { ... } set($value) { ... } }
    /// Returns (get_hook, set_hook)
    fn parse_property_hooks(&mut self, prop_name: &[u8]) -> ParseResult<(Option<Vec<Statement>>, Option<(Vec<u8>, Vec<Statement>)>)> {
        self.expect(&TokenKind::OpenBrace)?;
        let mut get_hook = None;
        let mut set_hook = None;

        while !matches!(self.peek(), TokenKind::CloseBrace | TokenKind::Eof) {
            // Skip optional attributes and modifiers: #[...] final, public, protected, private, &
            while matches!(self.peek(), TokenKind::AttributeOpen) {
                // Parse and discard attributes for now
                let _ = self.parse_attributes()?;
            }
            while matches!(self.peek(), TokenKind::Final | TokenKind::Public | TokenKind::Protected | TokenKind::Private | TokenKind::Ampersand) {
                self.advance();
            }
            // Look for "get" or "set" identifier
            let hook_name = match self.peek().clone() {
                TokenKind::Identifier(name) => name,
                _ => {
                    return Err(ParseError {
                        message: "expected 'get' or 'set' in property hook".into(),
                        span: self.span(),
                    });
                }
            };

            if hook_name.eq_ignore_ascii_case(b"get") {
                self.advance(); // consume 'get'
                if matches!(self.peek(), TokenKind::OpenBrace) {
                    // get { ... }
                    let body = self.parse_block()?;
                    get_hook = Some(body);
                } else if self.eat(&TokenKind::DoubleArrow) {
                    // get => expr;
                    let span = self.span();
                    let expr = self.parse_expression()?;
                    self.expect_semicolon()?;
                    let return_stmt = Statement {
                        kind: StmtKind::Return(Some(expr)),
                        span,
                    };
                    get_hook = Some(vec![return_stmt]);
                } else if self.eat(&TokenKind::Semicolon) {
                    // get; (abstract hook)
                    get_hook = Some(vec![]);
                } else {
                    return Err(ParseError {
                        message: "expected '{' or '=>' after 'get' in property hook".into(),
                        span: self.span(),
                    });
                }
            } else if hook_name.eq_ignore_ascii_case(b"set") {
                self.advance(); // consume 'set'
                // Optional parameter: set(Type $value) or set($value) or just set
                let param_name = if self.eat(&TokenKind::OpenParen) {
                    // Skip optional type hint before $value
                    // We need to handle: set($value), set(string $value), set(Type $value)
                    let pname = loop {
                        match self.peek().clone() {
                            TokenKind::Variable(name) => {
                                self.advance();
                                break name;
                            }
                            TokenKind::CloseParen => {
                                // set() with no param -- use default "value"
                                break b"value".to_vec();
                            }
                            _ => {
                                // Skip type hint tokens
                                self.advance();
                            }
                        }
                    };
                    self.expect(&TokenKind::CloseParen)?;
                    pname
                } else {
                    b"value".to_vec()
                };
                if matches!(self.peek(), TokenKind::OpenBrace) {
                    // set { ... } or set($value) { ... }
                    let body = self.parse_block()?;
                    set_hook = Some((param_name, body));
                } else if self.eat(&TokenKind::DoubleArrow) {
                    // set => expr; is shorthand for set { $this->propname = expr; }
                    let span = self.span();
                    let expr = self.parse_expression()?;
                    self.expect_semicolon()?;
                    // Generate: $this->propname = expr;
                    let this_expr = Expr { kind: ExprKind::Variable(b"this".to_vec()), span: span.clone() };
                    let prop_ident = Expr { kind: ExprKind::Identifier(prop_name.to_vec()), span: span.clone() };
                    let prop_access = Expr {
                        kind: ExprKind::PropertyAccess {
                            object: Box::new(this_expr),
                            property: Box::new(prop_ident),
                            nullsafe: false,
                        },
                        span: span.clone(),
                    };
                    let assign = Expr {
                        kind: ExprKind::Assign {
                            target: Box::new(prop_access),
                            value: Box::new(expr),
                        },
                        span: span.clone(),
                    };
                    let assign_stmt = Statement {
                        kind: StmtKind::Expression(assign),
                        span,
                    };
                    set_hook = Some((param_name, vec![assign_stmt]));
                } else if self.eat(&TokenKind::Semicolon) {
                    // set; (abstract hook)
                    set_hook = Some((param_name, vec![]));
                } else {
                    return Err(ParseError {
                        message: "expected '{' or '=>' after 'set' in property hook".into(),
                        span: self.span(),
                    });
                }
            } else {
                return Err(ParseError {
                    message: format!("expected 'get' or 'set' in property hook, got '{}'",
                        String::from_utf8_lossy(&hook_name)),
                    span: self.span(),
                });
            }
        }

        self.expect(&TokenKind::CloseBrace)?;
        Ok((get_hook, set_hook))
    }

    fn parse_try_catch(&mut self) -> ParseResult<Statement> {
        let span = self.span();
        self.advance(); // try
        let try_body = self.parse_block()?;

        let mut catches = Vec::new();
        while self.eat(&TokenKind::Catch) {
            self.expect(&TokenKind::OpenParen)?;

            let mut types = Vec::new();
            loop {
                // Parse qualified name: [\]Identifier[\Identifier]*
                let is_fqn = self.eat(&TokenKind::Backslash); // optional leading backslash
                let mut type_name = Vec::new();
                if is_fqn {
                    type_name.push(Vec::new()); // empty part marks fully qualified
                }
                loop {
                    match self.peek().clone() {
                        TokenKind::Identifier(name) => {
                            self.advance();
                            type_name.push(name);
                        }
                        _ if self.is_semi_reserved_keyword() => {
                            type_name.push(self.keyword_to_identifier());
                            self.advance();
                        }
                        _ if type_name.is_empty() => {
                            return Err(ParseError {
                                message: "expected exception class name".into(),
                                span: self.span(),
                            });
                        }
                        _ => break,
                    }
                    if !self.eat(&TokenKind::Backslash) {
                        break;
                    }
                }
                types.push(type_name);
                if !self.eat(&TokenKind::Pipe) {
                    break;
                }
            }

            let variable = if let TokenKind::Variable(name) = self.peek().clone() {
                self.advance();
                Some(name)
            } else {
                None
            };

            self.expect(&TokenKind::CloseParen)?;
            let body = self.parse_block()?;

            catches.push(CatchClause {
                types,
                variable,
                body,
            });
        }

        let finally_body = if self.eat(&TokenKind::Finally) {
            Some(self.parse_block()?)
        } else {
            None
        };

        Ok(Statement {
            kind: StmtKind::TryCatch {
                try_body,
                catches,
                finally_body,
            },
            span,
        })
    }

    fn parse_declare(&mut self) -> ParseResult<Statement> {
        let span = self.span();
        self.advance(); // declare
        self.expect(&TokenKind::OpenParen)?;

        let mut directives = Vec::new();
        loop {
            let name = match self.peek().clone() {
                TokenKind::Identifier(name) => {
                    self.advance();
                    name
                }
                _ => {
                    return Err(ParseError {
                        message: "expected directive name".into(),
                        span: self.span(),
                    });
                }
            };
            self.expect(&TokenKind::Assign)?;
            let value = self.parse_expression()?;
            directives.push((name, value));
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::CloseParen)?;

        let body = if matches!(self.peek(), TokenKind::OpenBrace) {
            Some(self.parse_block()?)
        } else {
            self.expect_semicolon()?;
            None
        };

        Ok(Statement {
            kind: StmtKind::Declare { directives, body },
            span,
        })
    }

    // ---- Expression parsing (Pratt parser) ----

    pub fn parse_expression(&mut self) -> ParseResult<Expr> {
        self.enter_depth()?;
        let result = self.parse_logical_or_low();
        self.leave_depth();
        result
    }

    /// Low-precedence 'or' (lower than assignment)
    fn parse_logical_or_low(&mut self) -> ParseResult<Expr> {
        let mut left = self.parse_logical_xor_low()?;
        while matches!(self.peek(), TokenKind::Or) {
            self.advance();
            let right = self.parse_logical_xor_low()?;
            left = Expr {
                span: left.span.merge(right.span),
                kind: ExprKind::BinaryOp {
                    op: BinaryOp::LogicalOr,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            };
        }
        Ok(left)
    }

    /// Low-precedence 'xor' (lower than assignment, higher than 'or')
    fn parse_logical_xor_low(&mut self) -> ParseResult<Expr> {
        let mut left = self.parse_logical_and_low()?;
        while matches!(self.peek(), TokenKind::Xor) {
            self.advance();
            let right = self.parse_logical_and_low()?;
            left = Expr {
                span: left.span.merge(right.span),
                kind: ExprKind::BinaryOp {
                    op: BinaryOp::LogicalXor,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            };
        }
        Ok(left)
    }

    /// Low-precedence 'and' (lower than assignment, higher than 'xor')
    fn parse_logical_and_low(&mut self) -> ParseResult<Expr> {
        let mut left = self.parse_assignment()?;
        while matches!(self.peek(), TokenKind::And) {
            self.advance();
            let right = self.parse_assignment()?;
            left = Expr {
                span: left.span.merge(right.span),
                kind: ExprKind::BinaryOp {
                    op: BinaryOp::LogicalAnd,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            };
        }
        Ok(left)
    }

    /// Check if an expression is a valid assignment target (l-value)
    fn is_lvalue(expr: &Expr) -> bool {
        matches!(
            &expr.kind,
            ExprKind::Variable(_)
                | ExprKind::ArrayAccess { .. }
                | ExprKind::PropertyAccess { .. }
                | ExprKind::StaticPropertyAccess { .. }
                | ExprKind::DynamicVariable(_)
        )
    }

    fn parse_assignment(&mut self) -> ParseResult<Expr> {
        let left = self.parse_null_coalesce()?;

        match self.peek().clone() {
            TokenKind::Assign => {
                self.advance();
                if self.eat(&TokenKind::Ampersand) {
                    let right = self.parse_assignment()?;
                    Ok(Expr {
                        span: left.span.merge(right.span),
                        kind: ExprKind::AssignRef {
                            target: Box::new(left),
                            value: Box::new(right),
                        },
                    })
                } else {
                    let right = self.parse_assignment()?;
                    // If left side is not a valid l-value but is a binary op whose right operand is,
                    // restructure: (expr OP $var) = val → expr OP ($var = val)
                    // This handles patterns like: null !== $key = key($arr)
                    let should_restructure = !Self::is_lvalue(&left)
                        && matches!(&left.kind, ExprKind::BinaryOp { right: r, .. } if Self::is_lvalue(r));
                    if should_restructure {
                        if let ExprKind::BinaryOp { op, left: bin_left, right: bin_right } = left.kind {
                            let assign = Expr {
                                span: bin_right.span.merge(right.span),
                                kind: ExprKind::Assign {
                                    target: bin_right,
                                    value: Box::new(right),
                                },
                            };
                            return Ok(Expr {
                                span: bin_left.span.merge(assign.span),
                                kind: ExprKind::BinaryOp {
                                    op,
                                    left: bin_left,
                                    right: Box::new(assign),
                                },
                            });
                        }
                    }
                    Ok(Expr {
                        span: left.span.merge(right.span),
                        kind: ExprKind::Assign {
                            target: Box::new(left),
                            value: Box::new(right),
                        },
                    })
                }
            }
            TokenKind::PlusAssign => {
                self.advance();
                let right = self.parse_assignment()?;
                Ok(Expr {
                    span: left.span.merge(right.span),
                    kind: ExprKind::CompoundAssign {
                        op: BinaryOp::Add,
                        target: Box::new(left),
                        value: Box::new(right),
                    },
                })
            }
            TokenKind::MinusAssign => {
                self.advance();
                let right = self.parse_assignment()?;
                Ok(Expr {
                    span: left.span.merge(right.span),
                    kind: ExprKind::CompoundAssign {
                        op: BinaryOp::Sub,
                        target: Box::new(left),
                        value: Box::new(right),
                    },
                })
            }
            TokenKind::StarAssign => {
                self.advance();
                let right = self.parse_assignment()?;
                Ok(Expr {
                    span: left.span.merge(right.span),
                    kind: ExprKind::CompoundAssign {
                        op: BinaryOp::Mul,
                        target: Box::new(left),
                        value: Box::new(right),
                    },
                })
            }
            TokenKind::SlashAssign => {
                self.advance();
                let right = self.parse_assignment()?;
                Ok(Expr {
                    span: left.span.merge(right.span),
                    kind: ExprKind::CompoundAssign {
                        op: BinaryOp::Div,
                        target: Box::new(left),
                        value: Box::new(right),
                    },
                })
            }
            TokenKind::PercentAssign => {
                self.advance();
                let right = self.parse_assignment()?;
                Ok(Expr {
                    span: left.span.merge(right.span),
                    kind: ExprKind::CompoundAssign {
                        op: BinaryOp::Mod,
                        target: Box::new(left),
                        value: Box::new(right),
                    },
                })
            }
            TokenKind::PowAssign => {
                self.advance();
                let right = self.parse_assignment()?;
                Ok(Expr {
                    span: left.span.merge(right.span),
                    kind: ExprKind::CompoundAssign {
                        op: BinaryOp::Pow,
                        target: Box::new(left),
                        value: Box::new(right),
                    },
                })
            }
            TokenKind::DotAssign => {
                self.advance();
                let right = self.parse_assignment()?;
                Ok(Expr {
                    span: left.span.merge(right.span),
                    kind: ExprKind::CompoundAssign {
                        op: BinaryOp::Concat,
                        target: Box::new(left),
                        value: Box::new(right),
                    },
                })
            }
            TokenKind::AmpersandAssign => {
                self.advance();
                let right = self.parse_assignment()?;
                Ok(Expr {
                    span: left.span.merge(right.span),
                    kind: ExprKind::CompoundAssign {
                        op: BinaryOp::BitwiseAnd,
                        target: Box::new(left),
                        value: Box::new(right),
                    },
                })
            }
            TokenKind::PipeAssign => {
                self.advance();
                let right = self.parse_assignment()?;
                Ok(Expr {
                    span: left.span.merge(right.span),
                    kind: ExprKind::CompoundAssign {
                        op: BinaryOp::BitwiseOr,
                        target: Box::new(left),
                        value: Box::new(right),
                    },
                })
            }
            TokenKind::CaretAssign => {
                self.advance();
                let right = self.parse_assignment()?;
                Ok(Expr {
                    span: left.span.merge(right.span),
                    kind: ExprKind::CompoundAssign {
                        op: BinaryOp::BitwiseXor,
                        target: Box::new(left),
                        value: Box::new(right),
                    },
                })
            }
            TokenKind::ShiftLeftAssign => {
                self.advance();
                let right = self.parse_assignment()?;
                Ok(Expr {
                    span: left.span.merge(right.span),
                    kind: ExprKind::CompoundAssign {
                        op: BinaryOp::ShiftLeft,
                        target: Box::new(left),
                        value: Box::new(right),
                    },
                })
            }
            TokenKind::ShiftRightAssign => {
                self.advance();
                let right = self.parse_assignment()?;
                Ok(Expr {
                    span: left.span.merge(right.span),
                    kind: ExprKind::CompoundAssign {
                        op: BinaryOp::ShiftRight,
                        target: Box::new(left),
                        value: Box::new(right),
                    },
                })
            }
            TokenKind::NullCoalesceAssign => {
                self.advance();
                let right = self.parse_assignment()?;
                let span = left.span.merge(right.span);
                // $x ??= val  →  $x = $x ?? val
                Ok(Expr {
                    span,
                    kind: ExprKind::Assign {
                        target: Box::new(left.clone()),
                        value: Box::new(Expr {
                            span,
                            kind: ExprKind::NullCoalesce {
                                left: Box::new(left),
                                right: Box::new(right),
                            },
                        }),
                    },
                })
            }
            _ => Ok(left),
        }
    }

    fn parse_null_coalesce(&mut self) -> ParseResult<Expr> {
        let left = self.parse_ternary()?;
        if self.eat(&TokenKind::NullCoalesce) {
            let right = self.parse_null_coalesce()?; // right-associative
            Ok(Expr {
                span: left.span.merge(right.span),
                kind: ExprKind::NullCoalesce {
                    left: Box::new(left),
                    right: Box::new(right),
                },
            })
        } else {
            Ok(left)
        }
    }

    fn parse_ternary(&mut self) -> ParseResult<Expr> {
        let cond = self.parse_logical_or()?;
        if self.eat(&TokenKind::QuestionMark) {
            let if_true = if matches!(self.peek(), TokenKind::Colon) {
                None // short ternary: $a ?: $b
            } else {
                Some(Box::new(self.parse_expression()?))
            };
            self.expect(&TokenKind::Colon)?;
            let if_false = self.parse_ternary()?;
            Ok(Expr {
                span: cond.span.merge(if_false.span),
                kind: ExprKind::Ternary {
                    condition: Box::new(cond),
                    if_true,
                    if_false: Box::new(if_false),
                },
            })
        } else {
            Ok(cond)
        }
    }

    fn parse_logical_or(&mut self) -> ParseResult<Expr> {
        let mut left = self.parse_logical_and()?;
        while matches!(self.peek(), TokenKind::BooleanOr) {
            self.advance();
            let right = self.parse_logical_and()?;
            left = Expr {
                span: left.span.merge(right.span),
                kind: ExprKind::BinaryOp {
                    op: BinaryOp::BooleanOr,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            };
        }
        Ok(left)
    }

    fn parse_logical_and(&mut self) -> ParseResult<Expr> {
        let mut left = self.parse_bitwise_or()?;
        while matches!(self.peek(), TokenKind::BooleanAnd) {
            self.advance();
            let right = self.parse_bitwise_or()?;
            left = Expr {
                span: left.span.merge(right.span),
                kind: ExprKind::BinaryOp {
                    op: BinaryOp::BooleanAnd,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            };
        }
        Ok(left)
    }

    fn parse_bitwise_or(&mut self) -> ParseResult<Expr> {
        let mut left = self.parse_bitwise_xor()?;
        while matches!(self.peek(), TokenKind::Pipe) {
            self.advance();
            let right = self.parse_bitwise_xor()?;
            left = Expr {
                span: left.span.merge(right.span),
                kind: ExprKind::BinaryOp {
                    op: BinaryOp::BitwiseOr,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            };
        }
        Ok(left)
    }

    fn parse_bitwise_xor(&mut self) -> ParseResult<Expr> {
        let mut left = self.parse_bitwise_and()?;
        while matches!(self.peek(), TokenKind::Caret) {
            self.advance();
            let right = self.parse_bitwise_and()?;
            left = Expr {
                span: left.span.merge(right.span),
                kind: ExprKind::BinaryOp {
                    op: BinaryOp::BitwiseXor,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            };
        }
        Ok(left)
    }

    fn parse_bitwise_and(&mut self) -> ParseResult<Expr> {
        let mut left = self.parse_pipe()?;
        while matches!(self.peek(), TokenKind::Ampersand) {
            self.advance();
            let right = self.parse_equality()?;
            left = Expr {
                span: left.span.merge(right.span),
                kind: ExprKind::BinaryOp {
                    op: BinaryOp::BitwiseAnd,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            };
        }
        Ok(left)
    }

    /// Parse pipe operator (|>), left-associative
    /// Precedence between equality and comparison
    fn parse_pipe(&mut self) -> ParseResult<Expr> {
        let mut left = self.parse_equality()?;
        while matches!(self.peek(), TokenKind::PipeGreater) {
            self.advance();
            let right = self.parse_equality()?;
            left = Expr {
                span: left.span.merge(right.span),
                kind: ExprKind::Pipe {
                    value: Box::new(left),
                    callable: Box::new(right),
                },
            };
        }
        Ok(left)
    }

    fn parse_equality(&mut self) -> ParseResult<Expr> {
        let mut left = self.parse_comparison()?;
        loop {
            let op = match self.peek() {
                TokenKind::Equal => BinaryOp::Equal,
                TokenKind::Identical => BinaryOp::Identical,
                TokenKind::NotEqual => BinaryOp::NotEqual,
                TokenKind::NotIdentical => BinaryOp::NotIdentical,
                _ => break,
            };
            self.advance();
            let right = self.parse_comparison()?;
            left = Expr {
                span: left.span.merge(right.span),
                kind: ExprKind::BinaryOp {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            };
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> ParseResult<Expr> {
        let mut left = self.parse_shift()?;
        loop {
            let op = match self.peek() {
                TokenKind::Less => BinaryOp::Less,
                TokenKind::Greater => BinaryOp::Greater,
                TokenKind::LessEqual => BinaryOp::LessEqual,
                TokenKind::GreaterEqual => BinaryOp::GreaterEqual,
                TokenKind::Spaceship => BinaryOp::Spaceship,
                TokenKind::Instanceof => {
                    self.advance();
                    let right = self.parse_shift()?;
                    left = Expr {
                        span: left.span.merge(right.span),
                        kind: ExprKind::Instanceof {
                            expr: Box::new(left),
                            class: Box::new(right),
                        },
                    };
                    continue;
                }
                _ => break,
            };
            self.advance();
            let right = self.parse_shift()?;
            left = Expr {
                span: left.span.merge(right.span),
                kind: ExprKind::BinaryOp {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            };
        }
        Ok(left)
    }

    fn parse_shift(&mut self) -> ParseResult<Expr> {
        let mut left = self.parse_additive()?;
        loop {
            let op = match self.peek() {
                TokenKind::ShiftLeft => BinaryOp::ShiftLeft,
                TokenKind::ShiftRight => BinaryOp::ShiftRight,
                _ => break,
            };
            self.advance();
            let right = self.parse_additive()?;
            left = Expr {
                span: left.span.merge(right.span),
                kind: ExprKind::BinaryOp {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            };
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> ParseResult<Expr> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.peek() {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Sub,
                TokenKind::Dot => BinaryOp::Concat,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr {
                span: left.span.merge(right.span),
                kind: ExprKind::BinaryOp {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            };
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> ParseResult<Expr> {
        let mut left = self.parse_pow()?;
        loop {
            let op = match self.peek() {
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                TokenKind::Percent => BinaryOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_pow()?;
            left = Expr {
                span: left.span.merge(right.span),
                kind: ExprKind::BinaryOp {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            };
        }
        Ok(left)
    }

    fn parse_pow(&mut self) -> ParseResult<Expr> {
        let base = self.parse_unary()?;
        if self.eat(&TokenKind::Pow) {
            let exp = self.parse_pow()?; // right-associative
            Ok(Expr {
                span: base.span.merge(exp.span),
                kind: ExprKind::BinaryOp {
                    op: BinaryOp::Pow,
                    left: Box::new(base),
                    right: Box::new(exp),
                },
            })
        } else {
            Ok(base)
        }
    }

    fn parse_unary(&mut self) -> ParseResult<Expr> {
        let span = self.span();
        match self.peek().clone() {
            TokenKind::Minus => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::UnaryOp {
                        op: UnaryOp::Negate,
                        operand: Box::new(operand),
                        prefix: true,
                    },
                })
            }
            TokenKind::Plus => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::UnaryOp {
                        op: UnaryOp::Plus,
                        operand: Box::new(operand),
                        prefix: true,
                    },
                })
            }
            TokenKind::BooleanNot => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::UnaryOp {
                        op: UnaryOp::BooleanNot,
                        operand: Box::new(operand),
                        prefix: true,
                    },
                })
            }
            TokenKind::Tilde => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::UnaryOp {
                        op: UnaryOp::BitwiseNot,
                        operand: Box::new(operand),
                        prefix: true,
                    },
                })
            }
            TokenKind::Increment => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::UnaryOp {
                        op: UnaryOp::PreIncrement,
                        operand: Box::new(operand),
                        prefix: true,
                    },
                })
            }
            TokenKind::Decrement => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::UnaryOp {
                        op: UnaryOp::PreDecrement,
                        operand: Box::new(operand),
                        prefix: true,
                    },
                })
            }
            TokenKind::IntCast => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::Cast(CastType::Int, Box::new(operand)),
                })
            }
            TokenKind::FloatCast => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::Cast(CastType::Float, Box::new(operand)),
                })
            }
            TokenKind::StringCast => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::Cast(CastType::String, Box::new(operand)),
                })
            }
            TokenKind::BoolCast => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::Cast(CastType::Bool, Box::new(operand)),
                })
            }
            TokenKind::ArrayCast => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::Cast(CastType::Array, Box::new(operand)),
                })
            }
            TokenKind::ObjectCast => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::Cast(CastType::Object, Box::new(operand)),
                })
            }
            TokenKind::RealCast => {
                return Err(ParseError {
                    message: "The (real) cast has been removed, use (float) instead".to_string(),
                    span,
                });
            }
            TokenKind::At => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::Suppress(Box::new(operand)),
                })
            }
            TokenKind::Clone => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::Clone(Box::new(operand)),
                })
            }
            TokenKind::Print => {
                self.advance();
                let operand = self.parse_expression()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::Print(Box::new(operand)),
                })
            }
            TokenKind::Throw => {
                self.advance();
                let operand = self.parse_expression()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::ThrowExpr(Box::new(operand)),
                })
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> ParseResult<Expr> {
        let mut expr = self.parse_primary()?;

        loop {
            match self.peek().clone() {
                TokenKind::Increment => {
                    let span = self.span();
                    self.advance();
                    expr = Expr {
                        span: expr.span.merge(span),
                        kind: ExprKind::UnaryOp {
                            op: UnaryOp::PostIncrement,
                            operand: Box::new(expr),
                            prefix: false,
                        },
                    };
                }
                TokenKind::Decrement => {
                    let span = self.span();
                    self.advance();
                    expr = Expr {
                        span: expr.span.merge(span),
                        kind: ExprKind::UnaryOp {
                            op: UnaryOp::PostDecrement,
                            operand: Box::new(expr),
                            prefix: false,
                        },
                    };
                }
                TokenKind::OpenBracket => {
                    self.advance();
                    let index = if matches!(self.peek(), TokenKind::CloseBracket) {
                        None
                    } else {
                        Some(Box::new(self.parse_expression()?))
                    };
                    let end_span = self.span();
                    self.expect(&TokenKind::CloseBracket)?;
                    expr = Expr {
                        span: expr.span.merge(end_span),
                        kind: ExprKind::ArrayAccess {
                            array: Box::new(expr),
                            index,
                        },
                    };
                }
                TokenKind::Arrow => {
                    self.advance();
                    if matches!(self.peek(), TokenKind::OpenBrace) {
                        // $obj->{expr}
                        self.advance();
                        let prop = self.parse_expression()?;
                        self.expect(&TokenKind::CloseBrace)?;
                        if matches!(self.peek(), TokenKind::OpenParen) {
                            self.advance();
                            let args = self.parse_arguments()?;
                            let is_fcc = self.first_class_callable_flag;
                            let end_span = self.span();
                            self.expect(&TokenKind::CloseParen)?;
                            expr = if is_fcc {
                                Expr {
                                    span: expr.span.merge(end_span),
                                    kind: ExprKind::FirstClassCallable(CallableTarget::Method {
                                        object: Box::new(expr),
                                        method: Box::new(prop),
                                        nullsafe: false,
                                    }),
                                }
                            } else {
                                Expr {
                                    span: expr.span.merge(end_span),
                                    kind: ExprKind::MethodCall {
                                        object: Box::new(expr),
                                        method: Box::new(prop),
                                        args,
                                        nullsafe: false,
                                    },
                                }
                            };
                        } else {
                            expr = Expr {
                                span: expr.span.merge(prop.span),
                                kind: ExprKind::PropertyAccess {
                                    object: Box::new(expr),
                                    property: Box::new(prop),
                                    nullsafe: false,
                                },
                            };
                        }
                    } else if matches!(self.peek(), TokenKind::Variable(_)) {
                        // $obj->$var  (dynamic property access)
                        let prop_span = self.span();
                        let var_name = match self.peek().clone() {
                            TokenKind::Variable(name) => {
                                self.advance();
                                name
                            }
                            _ => unreachable!(),
                        };
                        let prop_expr = Expr {
                            kind: ExprKind::Variable(var_name),
                            span: prop_span,
                        };
                        if matches!(self.peek(), TokenKind::OpenParen) {
                            self.advance();
                            let args = self.parse_arguments()?;
                            let is_fcc = self.first_class_callable_flag;
                            let end_span = self.span();
                            self.expect(&TokenKind::CloseParen)?;
                            expr = if is_fcc {
                                Expr {
                                    span: expr.span.merge(end_span),
                                    kind: ExprKind::FirstClassCallable(CallableTarget::Method {
                                        object: Box::new(expr),
                                        method: Box::new(prop_expr),
                                        nullsafe: false,
                                    }),
                                }
                            } else {
                                Expr {
                                    span: expr.span.merge(end_span),
                                    kind: ExprKind::MethodCall {
                                        object: Box::new(expr),
                                        method: Box::new(prop_expr),
                                        args,
                                        nullsafe: false,
                                    },
                                }
                            };
                        } else {
                            expr = Expr {
                                span: expr.span.merge(prop_span),
                                kind: ExprKind::PropertyAccess {
                                    object: Box::new(expr),
                                    property: Box::new(prop_expr),
                                    nullsafe: false,
                                },
                            };
                        }
                    } else {
                        let prop_span = self.span();
                        let name = match self.peek().clone() {
                            TokenKind::Identifier(name) => {
                                self.advance();
                                name
                            }
                            _ if self.is_semi_reserved_keyword() => {
                                let kw = self.keyword_to_identifier();
                                self.advance();
                                kw
                            }
                            _ => {
                                return Err(ParseError {
                                    message: "expected property/method name".into(),
                                    span: self.span(),
                                });
                            }
                        };
                        if matches!(self.peek(), TokenKind::OpenParen) {
                            self.advance();
                            let args = self.parse_arguments()?;
                            let is_fcc = self.first_class_callable_flag;
                            let end_span = self.span();
                            self.expect(&TokenKind::CloseParen)?;
                            let method_expr = Expr {
                                kind: ExprKind::Identifier(name),
                                span: prop_span,
                            };
                            expr = if is_fcc {
                                Expr {
                                    span: expr.span.merge(end_span),
                                    kind: ExprKind::FirstClassCallable(CallableTarget::Method {
                                        object: Box::new(expr),
                                        method: Box::new(method_expr),
                                        nullsafe: false,
                                    }),
                                }
                            } else {
                                Expr {
                                    span: expr.span.merge(end_span),
                                    kind: ExprKind::MethodCall {
                                        object: Box::new(expr),
                                        method: Box::new(method_expr),
                                        args,
                                        nullsafe: false,
                                    },
                                }
                            };
                        } else {
                            expr = Expr {
                                span: expr.span.merge(prop_span),
                                kind: ExprKind::PropertyAccess {
                                    object: Box::new(expr),
                                    property: Box::new(Expr {
                                        kind: ExprKind::Identifier(name),
                                        span: prop_span,
                                    }),
                                    nullsafe: false,
                                },
                            };
                        }
                    }
                }
                TokenKind::NullsafeArrow => {
                    self.advance();
                    let prop_span = self.span();
                    let name = match self.peek().clone() {
                        TokenKind::Identifier(name) => {
                            self.advance();
                            name
                        }
                        _ if self.is_semi_reserved_keyword() => {
                            let kw = self.keyword_to_identifier();
                            self.advance();
                            kw
                        }
                        _ => {
                            return Err(ParseError {
                                message: "expected property/method name".into(),
                                span: self.span(),
                            });
                        }
                    };
                    if matches!(self.peek(), TokenKind::OpenParen) {
                        self.advance();
                        let args = self.parse_arguments()?;
                        let is_fcc = self.first_class_callable_flag;
                        let end_span = self.span();
                        self.expect(&TokenKind::CloseParen)?;
                        let method_expr = Expr {
                            kind: ExprKind::Identifier(name),
                            span: prop_span,
                        };
                        expr = if is_fcc {
                            Expr {
                                span: expr.span.merge(end_span),
                                kind: ExprKind::FirstClassCallable(CallableTarget::Method {
                                    object: Box::new(expr),
                                    method: Box::new(method_expr),
                                    nullsafe: true,
                                }),
                            }
                        } else {
                            Expr {
                                span: expr.span.merge(end_span),
                                kind: ExprKind::MethodCall {
                                    object: Box::new(expr),
                                    method: Box::new(method_expr),
                                    args,
                                    nullsafe: true,
                                },
                            }
                        };
                    } else {
                        expr = Expr {
                            span: expr.span.merge(prop_span),
                            kind: ExprKind::PropertyAccess {
                                object: Box::new(expr),
                                property: Box::new(Expr {
                                    kind: ExprKind::Identifier(name),
                                    span: prop_span,
                                }),
                                nullsafe: true,
                            },
                        };
                    }
                }
                TokenKind::DoubleColon => {
                    self.advance();
                    let member_span = self.span();
                    match self.peek().clone() {
                        TokenKind::Identifier(name)
                            if matches!(
                                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                                Some(TokenKind::OpenParen)
                            ) =>
                        {
                            self.advance(); // name
                            self.advance(); // (
                            let args = self.parse_arguments()?;
                            let is_fcc = self.first_class_callable_flag;
                            let end_span = self.span();
                            self.expect(&TokenKind::CloseParen)?;
                            expr = if is_fcc {
                                Expr {
                                    span: expr.span.merge(end_span),
                                    kind: ExprKind::FirstClassCallable(CallableTarget::StaticMethod {
                                        class: Box::new(expr),
                                        method: name,
                                    }),
                                }
                            } else {
                                Expr {
                                    span: expr.span.merge(end_span),
                                    kind: ExprKind::StaticMethodCall {
                                        class: Box::new(expr),
                                        method: name,
                                        args,
                                    },
                                }
                            };
                        }
                        TokenKind::Identifier(name) => {
                            self.advance();
                            expr = Expr {
                                span: expr.span.merge(member_span),
                                kind: ExprKind::ClassConstAccess {
                                    class: Box::new(expr),
                                    constant: name,
                                },
                            };
                        }
                        TokenKind::Class => {
                            self.advance();
                            expr = Expr {
                                span: expr.span.merge(member_span),
                                kind: ExprKind::ClassConstAccess {
                                    class: Box::new(expr),
                                    constant: b"class".to_vec(),
                                },
                            };
                        }
                        TokenKind::Variable(name) => {
                            self.advance();
                            // Check if followed by ( for dynamic static method call: Foo::$method()
                            if matches!(self.peek(), TokenKind::OpenParen) {
                                // Dynamic method call: resolve variable to get method name
                                self.advance(); // (
                                let args = self.parse_arguments()?;
                                let end_span = self.span();
                                self.expect(&TokenKind::CloseParen)?;
                                // Create a StaticMethodCall with the variable name as a dynamic expression
                                // We need to compile this as InitDynamicStaticCall
                                // For now, treat it as a function call: get the variable value,
                                // then call ClassName::$method_value()
                                expr = Expr {
                                    span: expr.span.merge(end_span),
                                    kind: ExprKind::DynamicStaticMethodCall {
                                        class: Box::new(expr),
                                        method: Box::new(Expr {
                                            kind: ExprKind::Variable(name),
                                            span: member_span,
                                        }),
                                        args,
                                    },
                                };
                            } else {
                                expr = Expr {
                                    span: expr.span.merge(member_span),
                                    kind: ExprKind::StaticPropertyAccess {
                                        class: Box::new(expr),
                                        property: name,
                                    },
                                };
                            }
                        }
                        TokenKind::VariableVariable(name) => {
                            // Test::$$bar - access static property whose name is in $bar
                            self.advance();
                            let var_expr = Expr {
                                kind: ExprKind::Variable(name),
                                span: member_span,
                            };
                            expr = Expr {
                                span: expr.span.merge(member_span),
                                kind: ExprKind::DynamicStaticPropertyAccess {
                                    class: Box::new(expr),
                                    property: Box::new(var_expr),
                                },
                            };
                        }
                        _ if self.is_semi_reserved_keyword() => {
                            let name = self.keyword_to_identifier();
                            self.advance();
                            // Check if followed by ( for static method call
                            if matches!(self.peek(), TokenKind::OpenParen) {
                                self.advance();
                                let args = self.parse_arguments()?;
                                let is_fcc = self.first_class_callable_flag;
                                let end_span = self.span();
                                self.expect(&TokenKind::CloseParen)?;
                                expr = if is_fcc {
                                    Expr {
                                        span: expr.span.merge(end_span),
                                        kind: ExprKind::FirstClassCallable(CallableTarget::StaticMethod {
                                            class: Box::new(expr),
                                            method: name,
                                        }),
                                    }
                                } else {
                                    Expr {
                                        span: expr.span.merge(end_span),
                                        kind: ExprKind::StaticMethodCall {
                                            class: Box::new(expr),
                                            method: name,
                                            args,
                                        },
                                    }
                                };
                            } else {
                                expr = Expr {
                                    span: expr.span.merge(member_span),
                                    kind: ExprKind::ClassConstAccess {
                                        class: Box::new(expr),
                                        constant: name,
                                    },
                                };
                            }
                        }
                        TokenKind::OpenBrace => {
                            // Dynamic class constant fetch: Foo::{$expr}
                            self.advance();
                            let inner = self.parse_expression()?;
                            let end_span = self.span();
                            self.expect(&TokenKind::CloseBrace)?;
                            expr = Expr {
                                span: expr.span.merge(end_span),
                                kind: ExprKind::DynamicClassConstAccess {
                                    class: Box::new(expr),
                                    constant: Box::new(inner),
                                },
                            };
                        }
                        _ => {
                            return Err(ParseError {
                                message: "expected member name after ::".into(),
                                span: self.span(),
                            });
                        }
                    }
                }
                TokenKind::OpenParen => {
                    // Function call (for variable functions: $func())
                    self.advance();
                    let args = self.parse_arguments()?;
                    let is_fcc = self.first_class_callable_flag;
                    let end_span = self.span();
                    self.expect(&TokenKind::CloseParen)?;
                    expr = if is_fcc {
                        Expr {
                            span: expr.span.merge(end_span),
                            kind: ExprKind::FirstClassCallable(CallableTarget::Function(Box::new(expr))),
                        }
                    } else {
                        Expr {
                            span: expr.span.merge(end_span),
                            kind: ExprKind::FunctionCall {
                                name: Box::new(expr),
                                args,
                            },
                        }
                    };
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    fn parse_primary(&mut self) -> ParseResult<Expr> {
        let span = self.span();
        match self.peek().clone() {
            TokenKind::AttributeOpen => {
                let attributes = self.parse_attributes()?;
                let span = self.span();
                match self.peek() {
                    TokenKind::Function => {
                        self.advance(); self.eat(&TokenKind::Ampersand);
                        self.expect(&TokenKind::OpenParen)?; let params = self.parse_params()?; self.expect(&TokenKind::CloseParen)?;
                        let use_vars = if matches!(self.peek(), TokenKind::Use) { self.advance(); self.expect(&TokenKind::OpenParen)?; let mut v = Vec::new(); loop { let br = self.eat(&TokenKind::Ampersand); match self.peek().clone() { TokenKind::Variable(n) => { self.advance(); v.push(ClosureUse{variable:n,by_ref:br}); } _ => break }; if !self.eat(&TokenKind::Comma) { break; } } self.expect(&TokenKind::CloseParen)?; v } else { Vec::new() };
                        let return_type = if self.eat(&TokenKind::Colon) { Some(self.parse_type_hint()?) } else { None };
                        let body = self.parse_block()?;
                        Ok(Expr { span, kind: ExprKind::Closure { is_static: false, params, use_vars, return_type, body, attributes } })
                    }
                    TokenKind::Fn => {
                        self.advance(); self.eat(&TokenKind::Ampersand);
                        self.expect(&TokenKind::OpenParen)?; let params = self.parse_params()?; self.expect(&TokenKind::CloseParen)?;
                        let return_type = if self.eat(&TokenKind::Colon) { Some(self.parse_type_hint()?) } else { None };
                        self.expect(&TokenKind::DoubleArrow)?; let body = self.parse_expression()?;
                        Ok(Expr { span, kind: ExprKind::ArrowFunction { is_static: false, params, return_type, body: Box::new(body), attributes } })
                    }
                    _ => Err(ParseError { message: "expected function or fn after attribute".into(), span: self.span() })
                }
            }
            TokenKind::LongNumber(n) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Int(n),
                    span,
                })
            }
            TokenKind::DoubleNumber(n) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Float(n),
                    span,
                })
            }
            TokenKind::ConstantString(s) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::String(s),
                    span,
                })
            }
            TokenKind::InterpolatedStringPart(s) => {
                self.advance();
                let mut parts = vec![StringPart::Literal(s)];
                // Collect remaining parts
                loop {
                    match self.peek().clone() {
                        TokenKind::Variable(name) => {
                            let var_span = self.span();
                            self.advance();
                            let mut expr = Expr {
                                kind: ExprKind::Variable(name),
                                span: var_span,
                            };
                            // Parse chained property/array access
                            loop {
                                if matches!(self.peek(), TokenKind::Arrow) {
                                    self.advance(); // consume ->
                                    if let TokenKind::Identifier(prop_name) = self.peek().clone() {
                                        self.advance();
                                        expr = Expr {
                                            kind: ExprKind::PropertyAccess {
                                                object: Box::new(expr),
                                                property: Box::new(Expr {
                                                    kind: ExprKind::Identifier(prop_name),
                                                    span: var_span,
                                                }),
                                                nullsafe: false,
                                            },
                                            span: var_span,
                                        };
                                    } else {
                                        break;
                                    }
                                } else if matches!(self.peek(), TokenKind::OpenBracket) {
                                    self.advance(); // consume [
                                    let index = match self.peek().clone() {
                                        TokenKind::LongNumber(n) => {
                                            self.advance();
                                            Expr {
                                                kind: ExprKind::Int(n),
                                                span: var_span,
                                            }
                                        }
                                        TokenKind::Variable(idx_name) => {
                                            self.advance();
                                            Expr {
                                                kind: ExprKind::Variable(idx_name),
                                                span: var_span,
                                            }
                                        }
                                        TokenKind::Identifier(key) => {
                                            self.advance();
                                            Expr {
                                                kind: ExprKind::String(key),
                                                span: var_span,
                                            }
                                        }
                                        TokenKind::ConstantString(key) => {
                                            self.advance();
                                            Expr {
                                                kind: ExprKind::String(key),
                                                span: var_span,
                                            }
                                        }
                                        _ => Expr {
                                            kind: ExprKind::Int(0),
                                            span: var_span,
                                        },
                                    };
                                    if matches!(self.peek(), TokenKind::CloseBracket) {
                                        self.advance(); // consume ]
                                    }
                                    expr = Expr {
                                        kind: ExprKind::ArrayAccess {
                                            array: Box::new(expr),
                                            index: Some(Box::new(index)),
                                        },
                                        span: var_span,
                                    };
                                } else {
                                    break;
                                }
                            }
                            parts.push(StringPart::Expr(expr));
                        }
                        TokenKind::InterpolatedStringPart(s) => {
                            self.advance();
                            parts.push(StringPart::Literal(s));
                        }
                        TokenKind::InterpolatedStringEnd(s) => {
                            self.advance();
                            if !s.is_empty() {
                                parts.push(StringPart::Literal(s));
                            }
                            break;
                        }
                        _ => break,
                    }
                }
                Ok(Expr {
                    kind: ExprKind::InterpolatedString(parts),
                    span,
                })
            }
            TokenKind::True => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::True,
                    span,
                })
            }
            TokenKind::False => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::False,
                    span,
                })
            }
            TokenKind::Null => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Null,
                    span,
                })
            }
            TokenKind::Variable(name) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Variable(name),
                    span,
                })
            }
            TokenKind::VariableVariable(name) => {
                self.advance();
                // $$var is DynamicVariable with the inner variable
                Ok(Expr {
                    kind: ExprKind::DynamicVariable(Box::new(Expr {
                        kind: ExprKind::Variable(name),
                        span,
                    })),
                    span,
                })
            }
            TokenKind::Identifier(name) => {
                self.advance();
                // Check for qualified name: Foo\Bar\Baz
                let mut full_name = name;
                while self.eat(&TokenKind::Backslash) {
                    if let TokenKind::Identifier(part) = self.peek().clone() {
                        self.advance();
                        full_name.push(b'\\');
                        full_name.extend_from_slice(&part);
                    } else {
                        break;
                    }
                }
                // Check if this is a function call
                if matches!(self.peek(), TokenKind::OpenParen) {
                    self.advance();
                    let args = self.parse_arguments()?;
                    let is_fcc = self.first_class_callable_flag;
                    let end_span = self.span();
                    self.expect(&TokenKind::CloseParen)?;
                    let name_expr = Expr {
                        kind: ExprKind::Identifier(full_name),
                        span,
                    };
                    if is_fcc {
                        Ok(Expr {
                            span: span.merge(end_span),
                            kind: ExprKind::FirstClassCallable(CallableTarget::Function(Box::new(name_expr))),
                        })
                    } else {
                        Ok(Expr {
                            span: span.merge(end_span),
                            kind: ExprKind::FunctionCall {
                                name: Box::new(name_expr),
                                args,
                            },
                        })
                    }
                } else {
                    Ok(Expr {
                        kind: ExprKind::Identifier(full_name),
                        span,
                    })
                }
            }
            TokenKind::OpenParen => {
                self.advance();
                let expr = self.parse_expression()?;
                self.expect(&TokenKind::CloseParen)?;
                Ok(expr)
            }
            TokenKind::OpenBracket => {
                self.advance();
                let mut elements = Vec::new();
                while !matches!(self.peek(), TokenKind::CloseBracket | TokenKind::Eof) {
                    // Handle empty elements (for list destructuring like [$a, , $c])
                    if matches!(self.peek(), TokenKind::Comma) {
                        elements.push(ArrayElement {
                            key: None,
                            value: Expr {
                                span,
                                kind: ExprKind::Null,
                            },
                            unpack: false,
                        });
                        self.advance(); // consume comma
                        continue;
                    }
                    if self.eat(&TokenKind::Ellipsis) {
                        let value = self.parse_expression()?;
                        elements.push(ArrayElement {
                            key: None,
                            value,
                            unpack: true,
                        });
                    } else {
                        // Handle &$var (reference in array)
                        let _is_ref = self.eat(&TokenKind::Ampersand);
                        let first = self.parse_expression()?;
                        if self.eat(&TokenKind::DoubleArrow) {
                            let _is_val_ref = self.eat(&TokenKind::Ampersand);
                            let value = self.parse_expression()?;
                            elements.push(ArrayElement {
                                key: Some(first),
                                value,
                                unpack: false,
                            });
                        } else {
                            elements.push(ArrayElement {
                                key: None,
                                value: first,
                                unpack: false,
                            });
                        }
                    }
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                let end_span = self.span();
                self.expect(&TokenKind::CloseBracket)?;
                Ok(Expr {
                    span: span.merge(end_span),
                    kind: ExprKind::Array(elements),
                })
            }
            TokenKind::Array => {
                self.advance();
                self.expect(&TokenKind::OpenParen)?;
                let mut elements = Vec::new();
                while !matches!(self.peek(), TokenKind::CloseParen | TokenKind::Eof) {
                    self.eat(&TokenKind::Ampersand); // ignore reference in array literal
                    let first = self.parse_expression()?;
                    if self.eat(&TokenKind::DoubleArrow) {
                        self.eat(&TokenKind::Ampersand); // ignore reference in value
                        let value = self.parse_expression()?;
                        elements.push(ArrayElement {
                            key: Some(first),
                            value,
                            unpack: false,
                        });
                    } else {
                        elements.push(ArrayElement {
                            key: None,
                            value: first,
                            unpack: false,
                        });
                    }
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                let end_span = self.span();
                self.expect(&TokenKind::CloseParen)?;
                Ok(Expr {
                    span: span.merge(end_span),
                    kind: ExprKind::Array(elements),
                })
            }
            TokenKind::New => {
                self.advance();
                let _anon_attrs = if matches!(self.peek(), TokenKind::AttributeOpen) { self.parse_attributes()? } else { Vec::new() };
                // Check for class modifiers before anonymous class: new readonly class, new abstract class, etc.
                let mut anon_modifiers = ClassModifiers::default();
                let mut has_anon_modifiers = false;
                loop {
                    match self.peek() {
                        TokenKind::Readonly if matches!(self.peek_at(1), TokenKind::Class | TokenKind::Readonly | TokenKind::Abstract | TokenKind::Final) => {
                            if anon_modifiers.is_readonly {
                                return Err(ParseError {
                                    message: "Multiple readonly modifiers are not allowed".into(),
                                    span: self.span(),
                                });
                            }
                            anon_modifiers.is_readonly = true;
                            has_anon_modifiers = true;
                            self.advance();
                        }
                        TokenKind::Abstract if matches!(self.peek_at(1), TokenKind::Class | TokenKind::Readonly | TokenKind::Abstract | TokenKind::Final) => {
                            anon_modifiers.is_abstract = true;
                            has_anon_modifiers = true;
                            self.advance();
                        }
                        TokenKind::Final if matches!(self.peek_at(1), TokenKind::Class | TokenKind::Readonly | TokenKind::Abstract | TokenKind::Final) => {
                            anon_modifiers.is_final = true;
                            has_anon_modifiers = true;
                            self.advance();
                        }
                        _ => break,
                    }
                }
                // Validate modifier combinations for anonymous classes
                if has_anon_modifiers && matches!(self.peek(), TokenKind::Class) {
                    if anon_modifiers.is_abstract {
                        return Err(ParseError {
                            message: "Cannot use the abstract modifier on an anonymous class".into(),
                            span: self.span(),
                        });
                    }
                    if anon_modifiers.is_final {
                        return Err(ParseError {
                            message: "Cannot use the final modifier on an anonymous class".into(),
                            span: self.span(),
                        });
                    }
                }
                // Parse class name (not a full primary expression - don't consume parens)
                let class_span = self.span();
                let class = match self.peek().clone() {
                    TokenKind::Identifier(name) => {
                        self.advance();
                        // Handle qualified names: Foo\Bar\Baz
                        let mut full_name = name;
                        while self.eat(&TokenKind::Backslash) {
                            if let TokenKind::Identifier(part) = self.peek().clone() {
                                self.advance();
                                full_name.push(b'\\');
                                full_name.extend_from_slice(&part);
                            }
                        }
                        Expr {
                            kind: ExprKind::Identifier(full_name),
                            span: class_span,
                        }
                    }
                    TokenKind::Static => {
                        self.advance();
                        Expr {
                            kind: ExprKind::Identifier(b"static".to_vec()),
                            span: class_span,
                        }
                    }
                    TokenKind::Variable(name) => {
                        self.advance();
                        let mut expr = Expr {
                            kind: ExprKind::Variable(name),
                            span: class_span,
                        };
                        // Handle array access, property access, etc. after variable
                        // e.g. new $arr[0](), new $obj->class()
                        loop {
                            match self.peek() {
                                TokenKind::OpenBracket => {
                                    self.advance();
                                    let index = if matches!(self.peek(), TokenKind::CloseBracket) {
                                        None
                                    } else {
                                        Some(self.parse_expression()?)
                                    };
                                    self.expect(&TokenKind::CloseBracket)?;
                                    let s = expr.span;
                                    expr = Expr {
                                        kind: ExprKind::ArrayAccess {
                                            array: Box::new(expr),
                                            index: index.map(Box::new),
                                        },
                                        span: s,
                                    };
                                }
                                TokenKind::Arrow => {
                                    self.advance();
                                    let prop = self.parse_primary()?;
                                    let s = expr.span;
                                    expr = Expr {
                                        kind: ExprKind::PropertyAccess {
                                            object: Box::new(expr),
                                            property: Box::new(prop),
                                            nullsafe: false,
                                        },
                                        span: s,
                                    };
                                }
                                TokenKind::DoubleColon => {
                                    self.advance();
                                    let member = match self.peek().clone() {
                                        TokenKind::Identifier(name) => {
                                            self.advance();
                                            name
                                        }
                                        TokenKind::Variable(name) => {
                                            self.advance();
                                            let mut prop = b"$".to_vec();
                                            prop.extend_from_slice(&name);
                                            prop
                                        }
                                        _ => {
                                            let p = self.parse_primary()?;
                                            match p.kind {
                                                ExprKind::Identifier(name) => name,
                                                _ => b"unknown".to_vec(),
                                            }
                                        }
                                    };
                                    let s = expr.span;
                                    expr = Expr {
                                        kind: ExprKind::StaticPropertyAccess {
                                            class: Box::new(expr),
                                            property: member,
                                        },
                                        span: s,
                                    };
                                }
                                _ => break,
                            }
                        }
                        expr
                    }
                    TokenKind::Class => {
                        // Anonymous class: new class { ... }
                        self.advance();
                        // Parse optional constructor args
                        let ctor_args = if matches!(self.peek(), TokenKind::OpenParen) {
                            self.advance();
                            let args = self.parse_arguments()?;
                            self.expect(&TokenKind::CloseParen)?;
                            args
                        } else {
                            Vec::new()
                        };
                        // Parse optional extends
                        let extends = if self.eat(&TokenKind::Extends) {
                            let mut name = Vec::new();
                            while matches!(
                                self.peek(),
                                TokenKind::Identifier(_) | TokenKind::Backslash
                            ) {
                                if let TokenKind::Identifier(part) = self.peek().clone() {
                                    if !name.is_empty() {
                                        name.push(b'\\');
                                    }
                                    name.extend_from_slice(&part);
                                    self.advance();
                                } else {
                                    self.advance(); // backslash
                                }
                            }
                            Some(name)
                        } else {
                            None
                        };
                        // Parse optional implements
                        let mut implements = Vec::new();
                        if self.eat(&TokenKind::Implements) {
                            loop {
                                let mut iface_name = Vec::new();
                                while matches!(
                                    self.peek(),
                                    TokenKind::Identifier(_) | TokenKind::Backslash
                                ) {
                                    if let TokenKind::Identifier(part) = self.peek().clone() {
                                        if !iface_name.is_empty() {
                                            iface_name.push(b'\\');
                                        }
                                        iface_name.extend_from_slice(&part);
                                        self.advance();
                                    } else {
                                        self.advance(); // backslash
                                    }
                                }
                                if !iface_name.is_empty() {
                                    implements.push(iface_name);
                                }
                                if !self.eat(&TokenKind::Comma) {
                                    break;
                                }
                            }
                        }
                        // Parse class body
                        let body = self.parse_class_body()?;
                        // Generate unique anonymous class name
                        self.anon_counter += 1;
                        let anon_name = format!("__anonymous_class_{}", self.anon_counter);
                        // Create the class declaration as a statement that needs to be
                        // prepended before this expression. We embed it in the New expression.
                        // The compiler handles this by checking for class@anonymous prefix.
                        let class_stmt = Statement {
                            kind: StmtKind::ClassDecl {
                                name: anon_name.as_bytes().to_vec(),
                                modifiers: anon_modifiers,
                                extends,
                                implements,
                                body,
                                enum_backing_type: None,
                                attributes: Vec::new(),
                            },
                            span,
                        };
                        // Push the class declaration and return the New expression directly.
                        // We must return here to bypass the outer args-parsing code below,
                        // since ctor_args have already been consumed from the token stream.
                        self.anon_class_stmts.push(class_stmt);
                        return Ok(Expr {
                            kind: ExprKind::New {
                                class: Box::new(Expr {
                                    kind: ExprKind::Identifier(anon_name.into_bytes()),
                                    span: class_span,
                                }),
                                args: ctor_args,
                            },
                            span,
                        });
                    }
                    TokenKind::Backslash => {
                        // Fully qualified: new \Foo\Bar()
                        // Prefix with \ to mark as fully qualified
                        self.advance();
                        let mut full_name = vec![b'\\'];
                        loop {
                            match self.peek().clone() {
                                TokenKind::Identifier(part) => {
                                    self.advance();
                                    if full_name.len() > 1 {
                                        full_name.push(b'\\');
                                    }
                                    full_name.extend_from_slice(&part);
                                }
                                _ => break,
                            }
                            if !self.eat(&TokenKind::Backslash) {
                                break;
                            }
                        }
                        Expr {
                            kind: ExprKind::Identifier(full_name),
                            span: class_span,
                        }
                    }
                    _ => self.parse_primary()?,
                };
                let args = if matches!(self.peek(), TokenKind::OpenParen) {
                    self.advance();
                    let args = self.parse_arguments()?;
                    self.expect(&TokenKind::CloseParen)?;
                    args
                } else {
                    Vec::new()
                };
                Ok(Expr {
                    span,
                    kind: ExprKind::New {
                        class: Box::new(class),
                        args,
                    },
                })
            }
            TokenKind::Isset => {
                self.advance();
                self.expect(&TokenKind::OpenParen)?;
                let mut exprs = vec![self.parse_expression()?];
                while self.eat(&TokenKind::Comma) {
                    exprs.push(self.parse_expression()?);
                }
                self.expect(&TokenKind::CloseParen)?;
                Ok(Expr {
                    span,
                    kind: ExprKind::Isset(exprs),
                })
            }
            TokenKind::Empty => {
                self.advance();
                self.expect(&TokenKind::OpenParen)?;
                let expr = self.parse_expression()?;
                self.expect(&TokenKind::CloseParen)?;
                Ok(Expr {
                    span,
                    kind: ExprKind::Empty(Box::new(expr)),
                })
            }
            TokenKind::Exit => {
                self.advance();
                let value = if matches!(self.peek(), TokenKind::OpenParen) {
                    self.advance();
                    let v = if matches!(self.peek(), TokenKind::CloseParen) {
                        None
                    } else {
                        Some(Box::new(self.parse_expression()?))
                    };
                    self.expect(&TokenKind::CloseParen)?;
                    v
                } else {
                    None
                };
                Ok(Expr {
                    span,
                    kind: ExprKind::Exit(value),
                })
            }
            TokenKind::Eval => {
                self.advance();
                self.expect(&TokenKind::OpenParen)?;
                let expr = self.parse_expression()?;
                self.expect(&TokenKind::CloseParen)?;
                Ok(Expr {
                    span,
                    kind: ExprKind::Eval(Box::new(expr)),
                })
            }
            TokenKind::Function => {
                // Anonymous function
                self.advance();
                // Optional & for reference return
                let _by_ref_return = self.eat(&TokenKind::Ampersand);
                self.expect(&TokenKind::OpenParen)?;
                let params = self.parse_params()?;
                self.expect(&TokenKind::CloseParen)?;

                let use_vars = if matches!(self.peek(), TokenKind::Use) {
                    self.advance();
                    self.expect(&TokenKind::OpenParen)?;
                    let mut vars = Vec::new();
                    loop {
                        let by_ref = self.eat(&TokenKind::Ampersand);
                        let name = match self.peek().clone() {
                            TokenKind::Variable(name) => {
                                self.advance();
                                name
                            }
                            _ => {
                                // Allow trailing comma: if we already have vars, break
                                if !vars.is_empty() && !by_ref {
                                    break;
                                }
                                return Err(ParseError {
                                    message: "expected variable name in use clause".into(),
                                    span: self.span(),
                                });
                            }
                        };
                        vars.push(ClosureUse {
                            variable: name,
                            by_ref,
                        });
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(&TokenKind::CloseParen)?;
                    vars
                } else {
                    Vec::new()
                };

                let return_type = if self.eat(&TokenKind::Colon) {
                    Some(self.parse_type_hint()?)
                } else {
                    None
                };

                let body = self.parse_block()?;

                Ok(Expr {
                    span,
                    kind: ExprKind::Closure {
                        is_static: false,
                        params,
                        use_vars,
                        return_type,
                        body,
                        attributes: Vec::new(),
                    },
                })
            }
            TokenKind::Fn
                if !matches!(
                    self.tokens.get(self.pos + 1).map(|t| &t.kind),
                    Some(TokenKind::Backslash)
                ) =>
            {
                self.advance();
                // Optional & for reference return
                let _by_ref_return = self.eat(&TokenKind::Ampersand);
                self.expect(&TokenKind::OpenParen)?;
                let params = self.parse_params()?;
                self.expect(&TokenKind::CloseParen)?;
                let return_type = if self.eat(&TokenKind::Colon) {
                    Some(self.parse_type_hint()?)
                } else {
                    None
                };
                self.expect(&TokenKind::DoubleArrow)?;
                let body = self.parse_expression()?;
                Ok(Expr {
                    span,
                    kind: ExprKind::ArrowFunction {
                        is_static: false,
                        params,
                        return_type,
                        body: Box::new(body),
                        attributes: Vec::new(),
                    },
                })
            }
            TokenKind::Static
                if matches!(
                    self.tokens.get(self.pos + 1).map(|t| &t.kind),
                    Some(TokenKind::Function | TokenKind::Fn)
                ) =>
            {
                self.advance(); // static
                if matches!(self.peek(), TokenKind::Fn) {
                    self.advance();
                    let _by_ref_return = self.eat(&TokenKind::Ampersand);
                    self.expect(&TokenKind::OpenParen)?;
                    let params = self.parse_params()?;
                    self.expect(&TokenKind::CloseParen)?;
                    let return_type = if self.eat(&TokenKind::Colon) {
                        Some(self.parse_type_hint()?)
                    } else {
                        None
                    };
                    self.expect(&TokenKind::DoubleArrow)?;
                    let body = self.parse_expression()?;
                    Ok(Expr {
                        span,
                        kind: ExprKind::ArrowFunction {
                            is_static: true,
                            params,
                            return_type,
                            body: Box::new(body),
                            attributes: Vec::new(),
                        },
                    })
                } else {
                    self.advance(); // function
                    let _by_ref_return = self.eat(&TokenKind::Ampersand);
                    self.expect(&TokenKind::OpenParen)?;
                    let params = self.parse_params()?;
                    self.expect(&TokenKind::CloseParen)?;

                    // Parse use clause for static closures
                    let use_vars = if matches!(self.peek(), TokenKind::Use) {
                        self.advance();
                        self.expect(&TokenKind::OpenParen)?;
                        let mut vars = Vec::new();
                        loop {
                            let by_ref = self.eat(&TokenKind::Ampersand);
                            let name = match self.peek().clone() {
                                TokenKind::Variable(name) => {
                                    self.advance();
                                    name
                                }
                                _ => break,
                            };
                            vars.push(ClosureUse {
                                variable: name,
                                by_ref,
                            });
                            if !self.eat(&TokenKind::Comma) {
                                break;
                            }
                        }
                        self.expect(&TokenKind::CloseParen)?;
                        vars
                    } else {
                        Vec::new()
                    };
                    let return_type = if self.eat(&TokenKind::Colon) {
                        Some(self.parse_type_hint()?)
                    } else {
                        None
                    };
                    let body = self.parse_block()?;
                    Ok(Expr {
                        span,
                        kind: ExprKind::Closure {
                            is_static: true,
                            params,
                            use_vars,
                            return_type,
                            body,
                            attributes: Vec::new(),
                        },
                    })
                }
            }
            TokenKind::Match => {
                self.advance();
                self.expect(&TokenKind::OpenParen)?;
                let subject = self.parse_expression()?;
                self.expect(&TokenKind::CloseParen)?;
                self.expect(&TokenKind::OpenBrace)?;

                let mut arms = Vec::new();
                let mut has_default = false;
                while !matches!(self.peek(), TokenKind::CloseBrace | TokenKind::Eof) {
                    if self.eat(&TokenKind::Default) {
                        if has_default {
                            return Err(ParseError {
                                message: "Match expressions may only contain one default arm"
                                    .into(),
                                span: self.span(),
                            });
                        }
                        has_default = true;
                        // Allow trailing comma: default, =>
                        self.eat(&TokenKind::Comma);
                        self.expect(&TokenKind::DoubleArrow)?;
                        let body = self.parse_expression()?;
                        arms.push(MatchArm {
                            conditions: None,
                            body,
                        });
                    } else {
                        let mut conditions = vec![self.parse_expression()?];
                        while self.eat(&TokenKind::Comma) {
                            if matches!(self.peek(), TokenKind::DoubleArrow) {
                                break; // trailing comma
                            }
                            conditions.push(self.parse_expression()?);
                        }
                        self.expect(&TokenKind::DoubleArrow)?;
                        let body = self.parse_expression()?;
                        arms.push(MatchArm {
                            conditions: Some(conditions),
                            body,
                        });
                    }
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(&TokenKind::CloseBrace)?;

                Ok(Expr {
                    span,
                    kind: ExprKind::Match {
                        subject: Box::new(subject),
                        arms,
                    },
                })
            }
            TokenKind::Include => {
                self.advance();
                let path = self.parse_expression()?;
                Ok(Expr {
                    span,
                    kind: ExprKind::Include {
                        kind: IncludeKind::Include,
                        path: Box::new(path),
                    },
                })
            }
            TokenKind::IncludeOnce => {
                self.advance();
                let path = self.parse_expression()?;
                Ok(Expr {
                    span,
                    kind: ExprKind::Include {
                        kind: IncludeKind::IncludeOnce,
                        path: Box::new(path),
                    },
                })
            }
            TokenKind::Require => {
                self.advance();
                let path = self.parse_expression()?;
                Ok(Expr {
                    span,
                    kind: ExprKind::Include {
                        kind: IncludeKind::Require,
                        path: Box::new(path),
                    },
                })
            }
            TokenKind::RequireOnce => {
                self.advance();
                let path = self.parse_expression()?;
                Ok(Expr {
                    span,
                    kind: ExprKind::Include {
                        kind: IncludeKind::RequireOnce,
                        path: Box::new(path),
                    },
                })
            }
            TokenKind::Yield => {
                self.advance();
                if matches!(
                    self.peek(),
                    TokenKind::Semicolon | TokenKind::CloseParen | TokenKind::CloseBracket
                ) {
                    Ok(Expr {
                        span,
                        kind: ExprKind::Yield(None, None),
                    })
                } else {
                    let first = self.parse_expression()?;
                    // Check for yield $key => $value
                    if matches!(self.peek(), TokenKind::DoubleArrow) {
                        self.advance(); // consume =>
                        let value = self.parse_expression()?;
                        Ok(Expr {
                            span,
                            kind: ExprKind::Yield(Some(Box::new(value)), Some(Box::new(first))),
                        })
                    } else {
                        Ok(Expr {
                            span,
                            kind: ExprKind::Yield(Some(Box::new(first)), None),
                        })
                    }
                }
            }
            TokenKind::YieldFrom => {
                self.advance();
                let value = self.parse_expression()?;
                Ok(Expr {
                    span,
                    kind: ExprKind::YieldFrom(Box::new(value)),
                })
            }
            TokenKind::Ellipsis => {
                self.advance();
                let expr = self.parse_expression()?;
                Ok(Expr {
                    span,
                    kind: ExprKind::Spread(Box::new(expr)),
                })
            }
            TokenKind::Backslash => {
                // Fully qualified name like \Exception, \Foo\Bar
                // Prefix with \ to mark as fully qualified for the compiler
                self.advance();
                let mut name = vec![b'\\'];
                loop {
                    match self.peek().clone() {
                        TokenKind::Identifier(part) => {
                            self.advance();
                            if name.len() > 1 {
                                name.push(b'\\');
                            }
                            name.extend_from_slice(&part);
                        }
                        _ if self.is_semi_reserved_keyword() => {
                            if name.len() > 1 {
                                name.push(b'\\');
                            }
                            name.extend_from_slice(&self.keyword_to_identifier());
                            self.advance();
                        }
                        _ => break,
                    }
                    if !self.eat(&TokenKind::Backslash) {
                        break;
                    }
                }
                // Check if this is a function call or static access
                if matches!(self.peek(), TokenKind::OpenParen) {
                    self.advance();
                    let args = self.parse_arguments()?;
                    let is_fcc = self.first_class_callable_flag;
                    let end_span = self.span();
                    self.expect(&TokenKind::CloseParen)?;
                    let name_expr = Expr {
                        kind: ExprKind::Identifier(name),
                        span,
                    };
                    if is_fcc {
                        Ok(Expr {
                            span: span.merge(end_span),
                            kind: ExprKind::FirstClassCallable(CallableTarget::Function(Box::new(name_expr))),
                        })
                    } else {
                        Ok(Expr {
                            span: span.merge(end_span),
                            kind: ExprKind::FunctionCall {
                                name: Box::new(name_expr),
                                args,
                            },
                        })
                    }
                } else {
                    Ok(Expr {
                        kind: ExprKind::Identifier(name),
                        span,
                    })
                }
            }
            TokenKind::Static => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Identifier(b"static".to_vec()),
                    span,
                })
            }
            _ if self.is_semi_reserved_keyword() => {
                // Allow keywords used as identifiers in expression context
                let is_list = matches!(self.peek(), TokenKind::List);
                let mut full_name = self.keyword_to_identifier();
                self.advance();
                // Check for qualified name: keyword\Identifier\...
                while self.eat(&TokenKind::Backslash) {
                    match self.peek().clone() {
                        TokenKind::Identifier(part) => {
                            self.advance();
                            full_name.push(b'\\');
                            full_name.extend_from_slice(&part);
                        }
                        _ if self.is_semi_reserved_keyword() => {
                            full_name.push(b'\\');
                            full_name.extend_from_slice(&self.keyword_to_identifier());
                            self.advance();
                        }
                        _ => break,
                    }
                }
                if matches!(self.peek(), TokenKind::OpenParen) {
                    self.advance();
                    if is_list {
                        // Parse list() as an Array expression (supports keyed and empty slots)
                        let elements = self.parse_list_elements()?;
                        let end_span = self.span();
                        self.expect(&TokenKind::CloseParen)?;
                        Ok(Expr {
                            span: span.merge(end_span),
                            kind: ExprKind::Array(elements),
                        })
                    } else {
                        let args = self.parse_arguments()?;
                        let is_fcc = self.first_class_callable_flag;
                        let end_span = self.span();
                        self.expect(&TokenKind::CloseParen)?;
                        let name_expr = Expr {
                            kind: ExprKind::Identifier(full_name),
                            span,
                        };
                        if is_fcc {
                            Ok(Expr {
                                span: span.merge(end_span),
                                kind: ExprKind::FirstClassCallable(CallableTarget::Function(Box::new(name_expr))),
                            })
                        } else {
                            Ok(Expr {
                                span: span.merge(end_span),
                                kind: ExprKind::FunctionCall {
                                    name: Box::new(name_expr),
                                    args,
                                },
                            })
                        }
                    }
                } else {
                    Ok(Expr {
                        kind: ExprKind::Identifier(full_name),
                        span,
                    })
                }
            }
            _ => Err(ParseError {
                message: format!("syntax error, unexpected {}", self.peek().to_php_unexpected()),
                span,
            }),
        }
    }

    fn parse_arguments(&mut self) -> ParseResult<Vec<Argument>> {
        self.first_class_callable_flag = false;
        let mut args = Vec::new();
        if matches!(self.peek(), TokenKind::CloseParen) {
            return Ok(args);
        }

        // First-class callable syntax: foo(...)
        if matches!(self.peek(), TokenKind::Ellipsis)
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|t| matches!(t.kind, TokenKind::CloseParen))
        {
            self.advance(); // consume ...
            self.first_class_callable_flag = true;
            // Return empty args - the caller should interpret this as a callable reference
            return Ok(args);
        }

        loop {
            let unpack = self.eat(&TokenKind::Ellipsis);

            // Check for named arguments: name: value
            // PHP allows reserved keywords as named parameter names
            let name = {
                let maybe_name = match self.peek().clone() {
                    TokenKind::Identifier(ident) => Some(ident),
                    // Reserved keywords that can be used as named parameter names
                    TokenKind::Array => Some(b"array".to_vec()),
                    TokenKind::Callable => Some(b"callable".to_vec()),
                    TokenKind::Class => Some(b"class".to_vec()),
                    TokenKind::Static => Some(b"static".to_vec()),
                    TokenKind::Abstract => Some(b"abstract".to_vec()),
                    TokenKind::Final => Some(b"final".to_vec()),
                    TokenKind::Function => Some(b"function".to_vec()),
                    TokenKind::Fn => Some(b"fn".to_vec()),
                    TokenKind::Null => Some(b"null".to_vec()),
                    TokenKind::True => Some(b"true".to_vec()),
                    TokenKind::False => Some(b"false".to_vec()),
                    TokenKind::If => Some(b"if".to_vec()),
                    TokenKind::Else => Some(b"else".to_vec()),
                    TokenKind::ElseIf => Some(b"elseif".to_vec()),
                    TokenKind::While => Some(b"while".to_vec()),
                    TokenKind::For => Some(b"for".to_vec()),
                    TokenKind::Foreach => Some(b"foreach".to_vec()),
                    TokenKind::Do => Some(b"do".to_vec()),
                    TokenKind::Switch => Some(b"switch".to_vec()),
                    TokenKind::Case => Some(b"case".to_vec()),
                    TokenKind::Default => Some(b"default".to_vec()),
                    TokenKind::Break => Some(b"break".to_vec()),
                    TokenKind::Continue => Some(b"continue".to_vec()),
                    TokenKind::Return => Some(b"return".to_vec()),
                    TokenKind::Try => Some(b"try".to_vec()),
                    TokenKind::Catch => Some(b"catch".to_vec()),
                    TokenKind::Finally => Some(b"finally".to_vec()),
                    TokenKind::Throw => Some(b"throw".to_vec()),
                    TokenKind::Yield => Some(b"yield".to_vec()),
                    TokenKind::YieldFrom => Some(b"yield".to_vec()),
                    TokenKind::Match => Some(b"match".to_vec()),
                    TokenKind::Enum => Some(b"enum".to_vec()),
                    TokenKind::New => Some(b"new".to_vec()),
                    TokenKind::Clone => Some(b"clone".to_vec()),
                    TokenKind::Extends => Some(b"extends".to_vec()),
                    TokenKind::Implements => Some(b"implements".to_vec()),
                    TokenKind::Interface => Some(b"interface".to_vec()),
                    TokenKind::Trait => Some(b"trait".to_vec()),
                    TokenKind::Namespace => Some(b"namespace".to_vec()),
                    TokenKind::Use => Some(b"use".to_vec()),
                    TokenKind::Const => Some(b"const".to_vec()),
                    TokenKind::Var => Some(b"var".to_vec()),
                    TokenKind::Public => Some(b"public".to_vec()),
                    TokenKind::Protected => Some(b"protected".to_vec()),
                    TokenKind::Private => Some(b"private".to_vec()),
                    TokenKind::Readonly => Some(b"readonly".to_vec()),
                    TokenKind::Global => Some(b"global".to_vec()),
                    TokenKind::Isset => Some(b"isset".to_vec()),
                    TokenKind::Unset => Some(b"unset".to_vec()),
                    TokenKind::Empty => Some(b"empty".to_vec()),
                    TokenKind::Include => Some(b"include".to_vec()),
                    TokenKind::IncludeOnce => Some(b"include_once".to_vec()),
                    TokenKind::Require => Some(b"require".to_vec()),
                    TokenKind::RequireOnce => Some(b"require_once".to_vec()),
                    TokenKind::Echo => Some(b"echo".to_vec()),
                    TokenKind::Print => Some(b"print".to_vec()),
                    TokenKind::Instanceof => Some(b"instanceof".to_vec()),
                    TokenKind::Insteadof => Some(b"insteadof".to_vec()),
                    TokenKind::As => Some(b"as".to_vec()),
                    TokenKind::And => Some(b"and".to_vec()),
                    TokenKind::Or => Some(b"or".to_vec()),
                    TokenKind::Xor => Some(b"xor".to_vec()),
                    TokenKind::Eval => Some(b"eval".to_vec()),
                    TokenKind::Exit => Some(b"exit".to_vec()),
                    TokenKind::Goto => Some(b"goto".to_vec()),
                    TokenKind::List => Some(b"list".to_vec()),
                    TokenKind::Declare => Some(b"declare".to_vec()),
                    _ => None,
                };
                if let Some(ident) = maybe_name {
                    if self
                        .tokens
                        .get(self.pos + 1)
                        .is_some_and(|t| t.kind == TokenKind::Colon)
                    {
                        self.advance(); // identifier/keyword
                        self.advance(); // colon
                        Some(ident)
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            let value = self.parse_expression()?;
            args.push(Argument {
                name,
                value,
                unpack,
            });

            if !self.eat(&TokenKind::Comma) {
                break;
            }
            // Allow trailing comma
            if matches!(self.peek(), TokenKind::CloseParen) {
                break;
            }
        }

        Ok(args)
    }

    /// Parse list() elements which allow empty slots and keyed syntax: list($a, , "key" => $c)
    fn parse_list_elements(&mut self) -> ParseResult<Vec<ArrayElement>> {
        let mut elements = Vec::new();
        if matches!(self.peek(), TokenKind::CloseParen) {
            return Ok(elements);
        }

        loop {
            // Handle empty slots (commas without values)
            if matches!(self.peek(), TokenKind::Comma | TokenKind::CloseParen) {
                let span = self.span();
                elements.push(ArrayElement {
                    key: None,
                    value: Expr {
                        kind: ExprKind::Null,
                        span,
                    },
                    unpack: false,
                });
                if self.eat(&TokenKind::Comma) {
                    if matches!(self.peek(), TokenKind::CloseParen) {
                        break;
                    }
                    continue;
                }
                break;
            }

            let first = self.parse_expression()?;
            if self.eat(&TokenKind::DoubleArrow) {
                // Keyed list: "key" => $var or "key" => list(...)
                let value = self.parse_expression()?;
                elements.push(ArrayElement {
                    key: Some(first),
                    value,
                    unpack: false,
                });
            } else {
                elements.push(ArrayElement {
                    key: None,
                    value: first,
                    unpack: false,
                });
            }

            if !self.eat(&TokenKind::Comma) {
                break;
            }
            // Allow trailing comma
            if matches!(self.peek(), TokenKind::CloseParen) {
                break;
            }
        }

        Ok(elements)
    }

    /// Check if current token is a semi-reserved keyword that can be used as a method/function name
    /// Check if the current position is at `(set)` for asymmetric visibility.
    /// Assumes peek() is already `(`.
    fn lookahead_is_set_modifier(&self) -> bool {
        if let TokenKind::Identifier(name) = self.peek_at(1) {
            name.eq_ignore_ascii_case(b"set") && matches!(self.peek_at(2), TokenKind::CloseParen)
        } else {
            false
        }
    }

    fn is_semi_reserved_keyword(&self) -> bool {
        matches!(
            self.peek(),
            TokenKind::List
                | TokenKind::Array
                | TokenKind::Callable
                | TokenKind::Static
                | TokenKind::Abstract
                | TokenKind::Final
                | TokenKind::Private
                | TokenKind::Protected
                | TokenKind::Public
                | TokenKind::Readonly
                | TokenKind::Clone
                | TokenKind::New
                | TokenKind::Throw
                | TokenKind::Yield
                | TokenKind::YieldFrom
                | TokenKind::Print
                | TokenKind::Echo
                | TokenKind::Isset
                | TokenKind::Unset
                | TokenKind::Empty
                | TokenKind::Match
                | TokenKind::Switch
                | TokenKind::Case
                | TokenKind::Default
                | TokenKind::Break
                | TokenKind::Continue
                | TokenKind::Return
                | TokenKind::If
                | TokenKind::Else
                | TokenKind::ElseIf
                | TokenKind::While
                | TokenKind::Do
                | TokenKind::For
                | TokenKind::Foreach
                | TokenKind::As
                | TokenKind::Try
                | TokenKind::Catch
                | TokenKind::Finally
                | TokenKind::Class
                | TokenKind::Interface
                | TokenKind::Extends
                | TokenKind::Implements
                | TokenKind::Trait
                | TokenKind::Const
                | TokenKind::Enum
                | TokenKind::Fn
                | TokenKind::Function
                | TokenKind::Namespace
                | TokenKind::Use
                | TokenKind::Var
                | TokenKind::Global
                | TokenKind::Goto
                | TokenKind::Instanceof
                | TokenKind::Insteadof
                | TokenKind::Null
                | TokenKind::True
                | TokenKind::False
                | TokenKind::And
                | TokenKind::Or
                | TokenKind::Xor
                | TokenKind::Exit
                | TokenKind::Eval
                | TokenKind::Include
                | TokenKind::IncludeOnce
                | TokenKind::Require
                | TokenKind::RequireOnce
                | TokenKind::Declare
                | TokenKind::EndDeclare
                | TokenKind::EndFor
                | TokenKind::EndForeach
                | TokenKind::EndIf
                | TokenKind::EndSwitch
                | TokenKind::EndWhile
        )
    }

    /// Convert a keyword token to its identifier bytes
    fn keyword_to_identifier(&self) -> Vec<u8> {
        // For tokens that have aliases (die/exit, include/include_once, require/require_once),
        // use span length to determine which keyword was actually written
        let span = self.span();
        let span_len = span.end - span.start;
        match self.peek() {
            TokenKind::List => b"list".to_vec(),
            TokenKind::Array => b"array".to_vec(),
            TokenKind::Callable => b"callable".to_vec(),
            TokenKind::Static => b"static".to_vec(),
            TokenKind::Abstract => b"abstract".to_vec(),
            TokenKind::Final => b"final".to_vec(),
            TokenKind::Private => b"private".to_vec(),
            TokenKind::Protected => b"protected".to_vec(),
            TokenKind::Public => b"public".to_vec(),
            TokenKind::Readonly => b"readonly".to_vec(),
            TokenKind::Clone => b"clone".to_vec(),
            TokenKind::New => b"new".to_vec(),
            TokenKind::Throw => b"throw".to_vec(),
            TokenKind::Yield => b"yield".to_vec(),
            TokenKind::YieldFrom => b"yield_from".to_vec(),
            TokenKind::Print => b"print".to_vec(),
            TokenKind::Echo => b"echo".to_vec(),
            TokenKind::Isset => b"isset".to_vec(),
            TokenKind::Unset => b"unset".to_vec(),
            TokenKind::Empty => b"empty".to_vec(),
            TokenKind::Match => b"match".to_vec(),
            TokenKind::Switch => b"switch".to_vec(),
            TokenKind::Case => b"case".to_vec(),
            TokenKind::Default => b"default".to_vec(),
            TokenKind::Break => b"break".to_vec(),
            TokenKind::Continue => b"continue".to_vec(),
            TokenKind::Return => b"return".to_vec(),
            TokenKind::If => b"if".to_vec(),
            TokenKind::Else => b"else".to_vec(),
            TokenKind::ElseIf => b"elseif".to_vec(),
            TokenKind::While => b"while".to_vec(),
            TokenKind::Do => b"do".to_vec(),
            TokenKind::For => b"for".to_vec(),
            TokenKind::Foreach => b"foreach".to_vec(),
            TokenKind::As => b"as".to_vec(),
            TokenKind::Try => b"try".to_vec(),
            TokenKind::Catch => b"catch".to_vec(),
            TokenKind::Finally => b"finally".to_vec(),
            TokenKind::Class => b"class".to_vec(),
            TokenKind::Interface => b"interface".to_vec(),
            TokenKind::Extends => b"extends".to_vec(),
            TokenKind::Implements => b"implements".to_vec(),
            TokenKind::Trait => b"trait".to_vec(),
            TokenKind::Const => b"const".to_vec(),
            TokenKind::Enum => b"enum".to_vec(),
            TokenKind::Fn => b"fn".to_vec(),
            TokenKind::Function => b"function".to_vec(),
            TokenKind::Namespace => b"namespace".to_vec(),
            TokenKind::Use => b"use".to_vec(),
            TokenKind::Var => b"var".to_vec(),
            TokenKind::Global => b"global".to_vec(),
            TokenKind::Goto => b"goto".to_vec(),
            TokenKind::Instanceof => b"instanceof".to_vec(),
            TokenKind::Insteadof => b"insteadof".to_vec(),
            TokenKind::Null => b"null".to_vec(),
            TokenKind::True => b"true".to_vec(),
            TokenKind::False => b"false".to_vec(),
            TokenKind::And => b"and".to_vec(),
            TokenKind::Or => b"or".to_vec(),
            TokenKind::Xor => b"xor".to_vec(),
            TokenKind::Exit => if span_len == 3 { b"die".to_vec() } else { b"exit".to_vec() },
            TokenKind::Eval => b"eval".to_vec(),
            TokenKind::Include => b"include".to_vec(),
            TokenKind::IncludeOnce => b"include_once".to_vec(),
            TokenKind::Require => b"require".to_vec(),
            TokenKind::RequireOnce => b"require_once".to_vec(),
            TokenKind::Declare => b"declare".to_vec(),
            TokenKind::EndDeclare => b"enddeclare".to_vec(),
            TokenKind::EndFor => b"endfor".to_vec(),
            TokenKind::EndForeach => b"endforeach".to_vec(),
            TokenKind::EndIf => b"endif".to_vec(),
            TokenKind::EndSwitch => b"endswitch".to_vec(),
            TokenKind::EndWhile => b"endwhile".to_vec(),
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse(source: &[u8]) -> ParseResult<Program> {
        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize();
        let mut parser = Parser::new(tokens);
        parser.parse()
    }

    #[test]
    fn test_echo_string() {
        let prog = parse(b"<?php echo \"hello\";").unwrap();
        assert_eq!(prog.statements.len(), 1);
        match &prog.statements[0].kind {
            StmtKind::Echo(exprs) => {
                assert_eq!(exprs.len(), 1);
                match &exprs[0].kind {
                    ExprKind::String(s) => assert_eq!(s, b"hello"),
                    other => panic!("expected String, got {:?}", other),
                }
            }
            other => panic!("expected Echo, got {:?}", other),
        }
    }

    #[test]
    fn test_variable_assignment() {
        let prog = parse(b"<?php $x = 42;").unwrap();
        match &prog.statements[0].kind {
            StmtKind::Expression(expr) => match &expr.kind {
                ExprKind::Assign { target, value } => {
                    match &target.kind {
                        ExprKind::Variable(name) => assert_eq!(name, b"x"),
                        other => panic!("expected Variable, got {:?}", other),
                    }
                    match &value.kind {
                        ExprKind::Int(42) => {}
                        other => panic!("expected Int(42), got {:?}", other),
                    }
                }
                other => panic!("expected Assign, got {:?}", other),
            },
            other => panic!("expected Expression, got {:?}", other),
        }
    }

    #[test]
    fn test_binary_ops() {
        let prog = parse(b"<?php $a + $b * $c;").unwrap();
        match &prog.statements[0].kind {
            StmtKind::Expression(expr) => match &expr.kind {
                ExprKind::BinaryOp { op, left, right } => {
                    assert_eq!(*op, BinaryOp::Add);
                    matches!(&left.kind, ExprKind::Variable(_));
                    matches!(&right.kind, ExprKind::BinaryOp { .. });
                }
                other => panic!("expected BinaryOp, got {:?}", other),
            },
            other => panic!("expected Expression, got {:?}", other),
        }
    }

    #[test]
    fn test_function_call() {
        let prog = parse(b"<?php strlen(\"hello\");").unwrap();
        match &prog.statements[0].kind {
            StmtKind::Expression(expr) => match &expr.kind {
                ExprKind::FunctionCall { name, args } => {
                    match &name.kind {
                        ExprKind::Identifier(name) => assert_eq!(name, b"strlen"),
                        other => panic!("expected Identifier, got {:?}", other),
                    }
                    assert_eq!(args.len(), 1);
                }
                other => panic!("expected FunctionCall, got {:?}", other),
            },
            other => panic!("expected Expression, got {:?}", other),
        }
    }

    #[test]
    fn test_if_else() {
        let prog =
            parse(b"<?php if ($x > 0) { echo \"positive\"; } else { echo \"non-positive\"; }")
                .unwrap();
        match &prog.statements[0].kind {
            StmtKind::If {
                condition,
                body,
                else_body,
                ..
            } => {
                matches!(&condition.kind, ExprKind::BinaryOp { .. });
                assert_eq!(body.len(), 1);
                assert!(else_body.is_some());
            }
            other => panic!("expected If, got {:?}", other),
        }
    }
}
