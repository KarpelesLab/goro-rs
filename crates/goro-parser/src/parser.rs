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
    /// Doc comment attached to the token at the same index, if any. The
    /// comment immediately precedes the token in source (with only whitespace
    /// between). When a declaration starts at index N, the parser may consume
    /// the doc comment via `take_doc_comment_at(N)`.
    token_doc_comments: Vec<Option<Vec<u8>>>,
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
    /// The source text (when given with `with_source`): keywords used as
    /// identifiers keep their spelling (`A::Case`, `$o->Default`).
    source: Vec<u8>,
    /// Pending extra properties from comma-separated property declarations
    pending_extra_properties: Vec<ClassMember>,
    /// Explicit parameter type of the last parsed `set(Type $v)` hook.
    last_set_hook_type: Option<TypeHint>,
    /// Whether the get hook parsed by the last parse_property_hooks() was `&get`.
    last_get_hook_by_ref: bool,
    last_hook_attrs: HookAttrs,
    /// A doc-comment captured at the start of an assignment statement that
    /// should be applied to a closure/arrow function on the right-hand side.
    /// Set in parse_statement() when we see `/** doc */ $x = function/fn/static
    /// function ...`, and consumed by parse_primary at the closure entry.
    pending_closure_doc: Option<Vec<u8>>,
    class_name_stack: Vec<Vec<u8>>,
    /// Stack of "is this class actually an interface?" for nested class parsing.
    is_interface_stack: Vec<bool>,
    /// Nesting of statements being parsed (1 = top level).
    stmt_nesting: u32,
}

const MAX_PARSE_DEPTH: u32 = 512;

impl Parser {
    /// `__halt_compiler();` at the top level: drop everything after it and
    /// resolve direct references to `__COMPILER_HALT_OFFSET__` (unqualified
    /// or `\`-qualified) to the byte offset right after the statement.
    fn apply_halt_compiler(tokens: &mut Vec<Token>, docs: &mut Vec<Option<Vec<u8>>>) {
        let mut depth: i32 = 0;
        let mut halt: Option<(usize, u32)> = None;
        for i in 0..tokens.len() {
            match &tokens[i].kind {
                TokenKind::OpenBrace => depth += 1,
                TokenKind::CloseBrace => depth -= 1,
                TokenKind::Identifier(n)
                    if depth == 0 && n.eq_ignore_ascii_case(b"__halt_compiler") =>
                {
                    if matches!(
                        tokens.get(i + 1).map(|t| &t.kind),
                        Some(TokenKind::OpenParen)
                    ) && matches!(
                        tokens.get(i + 2).map(|t| &t.kind),
                        Some(TokenKind::CloseParen)
                    ) && matches!(
                        tokens.get(i + 3).map(|t| &t.kind),
                        Some(TokenKind::Semicolon) | Some(TokenKind::CloseTag)
                    ) {
                        let is_member = i > 0
                            && matches!(
                                tokens[i - 1].kind,
                                TokenKind::Arrow
                                    | TokenKind::NullsafeArrow
                                    | TokenKind::DoubleColon
                                    | TokenKind::Function
                                    | TokenKind::Backslash
                            );
                        if !is_member {
                            halt = Some((i, tokens[i + 3].span.end));
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
        let Some((hi, offset)) = halt else { return };
        // Keep `__halt_compiler ( ) ;` and the Eof token; drop the rest.
        if let Some(eof) = tokens.last().cloned() {
            if tokens.len() > hi + 4 {
                tokens.truncate(hi + 4);
                docs.truncate(hi + 4);
                if eof.kind == TokenKind::Eof {
                    tokens.push(eof);
                    docs.push(None);
                }
            }
        }
        let mut j = 0;
        while j < hi.min(tokens.len()) {
            let is_const = matches!(&tokens[j].kind, TokenKind::Identifier(n) if n.as_slice() == b"__COMPILER_HALT_OFFSET__");
            if is_const {
                let prev = if j > 0 {
                    Some(&tokens[j - 1].kind)
                } else {
                    None
                };
                let prev2 = if j > 1 {
                    Some(&tokens[j - 2].kind)
                } else {
                    None
                };
                let member = matches!(
                    prev,
                    Some(TokenKind::Arrow)
                        | Some(TokenKind::NullsafeArrow)
                        | Some(TokenKind::DoubleColon)
                        | Some(TokenKind::Const)
                        | Some(TokenKind::Function)
                );
                if !member {
                    if matches!(prev, Some(TokenKind::Backslash)) {
                        // `\X` is the global constant; `ns\X` / `namespace\X` are not.
                        if !matches!(
                            prev2,
                            Some(TokenKind::Identifier(_)) | Some(TokenKind::Namespace)
                        ) {
                            tokens[j].kind = TokenKind::LongNumber(offset as i64);
                            tokens.remove(j - 1);
                            docs.remove(j - 1);
                            continue;
                        }
                    } else {
                        tokens[j].kind = TokenKind::LongNumber(offset as i64);
                    }
                }
            }
            j += 1;
        }
    }

    pub fn new(tokens: Vec<Token>) -> Self {
        // Strip DocComment tokens out of the stream and attach each to the
        // *next* non-doc token. token_doc_comments[i] is the (last) doc comment
        // immediately preceding tokens[i].
        let mut filtered: Vec<Token> = Vec::with_capacity(tokens.len());
        let mut doc_comments: Vec<Option<Vec<u8>>> = Vec::with_capacity(tokens.len());
        let mut pending_doc: Option<Vec<u8>> = None;
        for tok in tokens {
            if let TokenKind::DocComment(bytes) = &tok.kind {
                // Keep the most recent doc-comment if multiple appear in a row
                pending_doc = Some(bytes.clone());
                continue;
            }
            doc_comments.push(pending_doc.take());
            filtered.push(tok);
        }
        Self::apply_halt_compiler(&mut filtered, &mut doc_comments);
        Self {
            tokens: filtered,
            token_doc_comments: doc_comments,
            pos: 0,
            depth: 0,
            anon_class_stmts: Vec::new(),
            anon_counter: 0,
            first_class_callable_flag: false,
            pending_extra_constants: Vec::new(),
            source: Vec::new(),
            pending_extra_properties: Vec::new(),
            last_set_hook_type: None,
            last_get_hook_by_ref: false,
            last_hook_attrs: HookAttrs::default(),
            pending_closure_doc: None,
            class_name_stack: Vec::new(),
            is_interface_stack: Vec::new(),
            stmt_nesting: 0,
        }
    }

    /// Take and clear the doc comment attached to the current token (if any).
    fn take_doc_comment(&mut self) -> Option<Vec<u8>> {
        self.token_doc_comments
            .get_mut(self.pos)
            .and_then(|d| d.take())
    }

    /// Peek the doc comment at the current token without consuming it.
    #[allow(dead_code)]
    fn peek_doc_comment(&self) -> Option<&Vec<u8>> {
        self.token_doc_comments
            .get(self.pos)
            .and_then(|d| d.as_ref())
    }

    /// Attach a doc comment to a freshly-parsed Statement that represents a
    /// function/class/etc. declaration. Other statement kinds keep no doc
    /// comment.
    fn attach_stmt_doc_comment(stmt: &mut Statement, doc: Vec<u8>) {
        match &mut stmt.kind {
            StmtKind::FunctionDecl { doc_comment, .. }
            | StmtKind::ClassDecl { doc_comment, .. } => {
                *doc_comment = Some(doc);
            }
            _ => {}
        }
    }

    /// zend_compile_use_trait(): `static` parses as a class name but is
    /// rejected when compiled.
    fn static_trait_name_error(&self) -> ParseError {
        ParseError {
            message: "Cannot use \"static\" as trait name, as it is reserved".into(),
            span: self.current().span,
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

    /// True if tokens[start..end] is a single `( ... )` group (the opening
    /// paren at `start` is matched by the closing paren at `end - 1`).
    fn tokens_parenthesized(&self, start: usize, end: usize) -> bool {
        if end <= start + 1 || end > self.tokens.len() {
            return false;
        }
        if !matches!(self.tokens[start].kind, TokenKind::OpenParen) {
            return false;
        }
        let mut depth = 0i32;
        for i in start..end {
            match self.tokens[i].kind {
                TokenKind::OpenParen => depth += 1,
                TokenKind::CloseParen => {
                    depth -= 1;
                    if depth == 0 {
                        return i == end - 1;
                    }
                }
                _ => {}
            }
        }
        false
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
                message: format!(
                    "syntax error, unexpected {}, expecting {}",
                    self.peek().to_php_unexpected(),
                    expected.to_php_name()
                ),
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
                message: format!(
                    "syntax error, unexpected {}",
                    self.peek().to_php_unexpected()
                ),
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

    /// The first bracket nesting error the scanner would raise
    /// (enter_nesting / exit_nesting / check_nesting_at_end in
    /// zend_language_scanner.l): (token index, message, line).
    /// Bison's parser stack is limited to YYMAXDEPTH (10000) states; each
    /// open bracket still awaiting its closing one holds at least one, so
    /// nesting that deep fails with "memory exhausted" (reported at the
    /// line of the token that overflowed the stack).
    fn parser_stack_exhausted(&self) -> Option<u32> {
        const YYMAXDEPTH: usize = 10000;
        let mut depth = 0usize;
        for tok in &self.tokens {
            match tok.kind {
                TokenKind::OpenParen
                | TokenKind::OpenBracket
                | TokenKind::AttributeOpen
                | TokenKind::OpenBrace
                | TokenKind::DollarOpenBrace => {
                    depth += 1;
                    if depth >= YYMAXDEPTH {
                        return Some(tok.span.line);
                    }
                }
                TokenKind::CloseParen | TokenKind::CloseBracket | TokenKind::CloseBrace => {
                    depth = depth.saturating_sub(1)
                }
                _ => {}
            }
        }
        None
    }

    fn first_nesting_error(&self) -> Option<(usize, String, u32)> {
        let mut stack: Vec<(u8, u32)> = Vec::new();
        for (i, tok) in self.tokens.iter().enumerate() {
            let line = tok.span.line;
            let close = match &tok.kind {
                TokenKind::OpenParen => {
                    stack.push((b'(', line));
                    continue;
                }
                TokenKind::OpenBracket | TokenKind::AttributeOpen => {
                    stack.push((b'[', line));
                    continue;
                }
                TokenKind::OpenBrace | TokenKind::DollarOpenBrace => {
                    stack.push((b'{', line));
                    continue;
                }
                TokenKind::CloseParen => b')',
                TokenKind::CloseBracket => b']',
                TokenKind::CloseBrace => b'}',
                TokenKind::Eof => {
                    let (open, oline) = stack.last().copied()?;
                    let msg = if oline == line {
                        format!("Unclosed '{}'", open as char)
                    } else {
                        format!("Unclosed '{}' on line {}", open as char, oline)
                    };
                    return Some((i, msg, line));
                }
                _ => continue,
            };
            let Some((open, oline)) = stack.pop() else {
                return Some((i, format!("Unmatched '{}'", close as char), line));
            };
            let expected = match open {
                b'(' => b')',
                b'[' => b']',
                _ => b'}',
            };
            if close != expected {
                let msg = if oline == line {
                    format!(
                        "Unclosed '{}' does not match '{}'",
                        open as char, close as char
                    )
                } else {
                    format!(
                        "Unclosed '{}' on line {} does not match '{}'",
                        open as char, oline, close as char
                    )
                };
                return Some((i, msg, line));
            }
        }
        None
    }

    /// case_separator: ':' | ';' (the latter deprecated in PHP 8.5).
    fn expect_case_separator(&mut self) -> ParseResult<()> {
        if matches!(self.peek(), TokenKind::Semicolon) {
            let line = self.span().line;
            self.advance();
            crate::push_lexer_deprecated(
                "Case statements followed by a semicolon (;) are deprecated, use a colon (:) instead"
                    .to_string(),
                line,
            );
            return Ok(());
        }
        self.expect(&TokenKind::Colon).map(|_| ())
    }

    /// Parse a complete PHP program
    pub fn parse(&mut self) -> ParseResult<Program> {
        if let Some(line) = self.parser_stack_exhausted() {
            let mut span = self.current().span;
            span.line = line;
            return Err(ParseError {
                message: "memory exhausted".into(),
                span,
            });
        }
        let nesting = self.first_nesting_error();
        let result = self.parse_program();
        match (result, nesting) {
            // The scanner raises its error when the parser fetches that token:
            // it wins over a syntax error found at or after it.
            (Err(e), Some((idx, message, line))) if idx <= self.pos => {
                let mut span = e.span;
                span.line = line;
                Err(ParseError { message, span })
            }
            (r, _) => r,
        }
    }

    fn parse_program(&mut self) -> ParseResult<Program> {
        if let Some((message, line)) = crate::take_lexer_error() {
            let mut span = self
                .tokens
                .first()
                .map(|t| t.span)
                .unwrap_or(crate::token::Span {
                    start: 0,
                    end: 0,
                    line,
                });
            span.line = line;
            return Err(ParseError { message, span });
        }
        let mut statements = Vec::new();
        while !self.is_at_end() {
            let stmt = self.parse_statement()?;
            // Drain any anonymous class declarations collected during expression parsing
            if !self.anon_class_stmts.is_empty() {
                let anon_stmts: Vec<_> = std::mem::take(&mut self.anon_class_stmts);
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
        // zend_compile_attributes() reports `#[A(...)]` while compiling the
        // declaration, i.e. on the line of the token after the attributes.
        let mut fcc_arg = false;
        let mut group = 0u32;
        while matches!(self.peek(), TokenKind::AttributeOpen) {
            self.advance();
            group += 1;
            loop {
                let mut parts = Vec::new();
                let line = self.span().line;
                // A fully qualified name keeps its leading backslash.
                let fq = self.eat(&TokenKind::Backslash);
                let fq_prefix = |n: Vec<u8>| {
                    if fq {
                        [b"\\".as_slice(), &n].concat()
                    } else {
                        n
                    }
                };
                match self.peek().clone() {
                    TokenKind::Identifier(n) => {
                        parts.push(fq_prefix(n));
                        self.advance();
                    }
                    _ if self.is_semi_reserved_keyword() => {
                        parts.push(fq_prefix(self.keyword_to_identifier()));
                        self.advance();
                    }
                    TokenKind::Variable(v) => {
                        return Err(ParseError {
                            message: format!(
                                "syntax error, unexpected variable \"${}\"",
                                String::from_utf8_lossy(&v)
                            ),
                            span: self.span(),
                        });
                    }
                    _ => {
                        return Err(ParseError {
                            message: "expected attribute name".into(),
                            span: self.span(),
                        });
                    }
                }
                while matches!(self.peek(), TokenKind::Backslash) {
                    self.advance();
                    match self.peek().clone() {
                        TokenKind::Identifier(n) => {
                            parts.push(n);
                            self.advance();
                        }
                        _ if self.is_semi_reserved_keyword() => {
                            parts.push(self.keyword_to_identifier());
                            self.advance();
                        }
                        _ => break,
                    }
                }
                let has_args = matches!(self.peek(), TokenKind::OpenParen);
                let args = if has_args {
                    self.advance();
                    let a = self.parse_arguments()?;
                    fcc_arg |= self.first_class_callable_flag;
                    self.expect(&TokenKind::CloseParen)?;
                    a
                } else {
                    Vec::new()
                };
                attrs.push(Attribute {
                    name: parts,
                    args,
                    line,
                    group,
                    has_args,
                });
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
                if matches!(self.peek(), TokenKind::CloseBracket) {
                    break;
                }
            }
            self.expect(&TokenKind::CloseBracket)?;
        }
        if fcc_arg {
            return Err(ParseError {
                message: "Cannot create Closure as attribute argument".into(),
                span: self.span(),
            });
        }
        Ok(attrs)
    }
    fn parse_statement(&mut self) -> ParseResult<Statement> {
        self.stmt_nesting += 1;
        let r = self.parse_statement_inner();
        self.stmt_nesting -= 1;
        r
    }

    fn parse_statement_inner(&mut self) -> ParseResult<Statement> {
        let span = self.span();
        if self.stmt_nesting > 1
            && matches!(self.peek(), TokenKind::Identifier(n) if n.eq_ignore_ascii_case(b"__halt_compiler"))
            && matches!(self.peek_at(1), TokenKind::OpenParen)
        {
            return Err(ParseError {
                message: "__HALT_COMPILER() can only be used from the outermost scope".into(),
                span,
            });
        }
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
                } else if matches!(
                    self.peek(),
                    TokenKind::Eof
                        | TokenKind::CloseBrace
                        | TokenKind::Else
                        | TokenKind::ElseIf
                        | TokenKind::EndIf
                        | TokenKind::EndWhile
                        | TokenKind::EndFor
                        | TokenKind::EndForeach
                        | TokenKind::EndSwitch
                        | TokenKind::EndDeclare
                        | TokenKind::Case
                        | TokenKind::Default
                ) {
                    // <?php with no body / at end of file: emit a no-op so the
                    // overall parse succeeds (matches PHP, which accepts a file
                    // containing nothing but the opening tag).
                    Ok(Statement {
                        kind: StmtKind::Nop,
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
            TokenKind::AttributeOpen => {
                let doc = self.take_doc_comment();
                let attributes = self.parse_attributes()?;
                let r = match self.peek() {
                    TokenKind::Function => self.parse_function_decl_with_attrs(attributes),
                    TokenKind::Class
                    | TokenKind::Abstract
                    | TokenKind::Final
                    | TokenKind::Interface
                    | TokenKind::Trait
                    | TokenKind::Enum => self.parse_class_decl_with_attrs(attributes),
                    TokenKind::Readonly
                        if matches!(
                            self.peek_at(1),
                            TokenKind::Class
                                | TokenKind::Enum
                                | TokenKind::Abstract
                                | TokenKind::Final
                                | TokenKind::Function
                                | TokenKind::Trait
                                | TokenKind::Interface
                                | TokenKind::Readonly
                        ) =>
                    {
                        self.parse_class_decl_with_attrs(attributes)
                    }
                    TokenKind::Const => self.parse_const_with_attrs(attributes),
                    _ => self.parse_statement(),
                };
                if let Some(d) = doc {
                    return r.map(|mut s| {
                        Self::attach_stmt_doc_comment(&mut s, d);
                        s
                    });
                }
                r
            }
            // `function (...) {...};` / `function &(...)` at statement start
            // is a closure expression statement, not a declaration.
            TokenKind::Function
                if !matches!(self.peek_at(1), TokenKind::OpenParen)
                    && !(matches!(self.peek_at(1), TokenKind::Ampersand)
                        && matches!(self.peek_at(2), TokenKind::OpenParen)) =>
            {
                let doc = self.take_doc_comment();
                let r = self.parse_function_decl_with_attrs(Vec::new());
                if let Some(d) = doc {
                    return r.map(|mut s| {
                        Self::attach_stmt_doc_comment(&mut s, d);
                        s
                    });
                }
                r
            }
            TokenKind::Class
            | TokenKind::Abstract
            | TokenKind::Final
            | TokenKind::Interface
            | TokenKind::Trait
            | TokenKind::Enum => {
                let doc = self.take_doc_comment();
                let r = self.parse_class_decl_with_attrs(Vec::new());
                if let Some(d) = doc {
                    return r.map(|mut s| {
                        Self::attach_stmt_doc_comment(&mut s, d);
                        s
                    });
                }
                r
            }
            TokenKind::Readonly => {
                // readonly can be a class modifier (readonly class Foo {}) or a function call
                // Check if followed by class/enum/abstract/final/function
                if matches!(
                    self.peek_at(1),
                    TokenKind::Class
                        | TokenKind::Enum
                        | TokenKind::Abstract
                        | TokenKind::Final
                        | TokenKind::Function
                        | TokenKind::Trait
                        | TokenKind::Interface
                        | TokenKind::Readonly
                ) {
                    let doc = self.take_doc_comment();
                    let r = self.parse_class_decl_with_attrs(Vec::new());
                    if let Some(d) = doc {
                        return r.map(|mut s| {
                            Self::attach_stmt_doc_comment(&mut s, d);
                            s
                        });
                    }
                    r
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
                                kind: ExprKind::FirstClassCallable(CallableTarget::Function(
                                    Box::new(name_expr),
                                )),
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
            TokenKind::Const => self.parse_const_with_attrs(Vec::new()),
            TokenKind::Namespace => {
                self.advance();
                // zend_compile_namespace(): `namespace namespace;` parses (the
                // keyword is a valid identifier) but is rejected.
                if matches!(self.peek(), TokenKind::Namespace)
                    && matches!(self.peek_at(1), TokenKind::Semicolon | TokenKind::OpenBrace)
                {
                    let span = self.span();
                    let text = self
                        .source
                        .get(span.start as usize..span.end as usize)
                        .map(|t| String::from_utf8_lossy(t).into_owned())
                        .unwrap_or_else(|| "namespace".into());
                    return Err(ParseError {
                        message: format!("Cannot use '{}' as namespace name", text),
                        span: self.current().span,
                    });
                }
                // namespace Name\Space;  or  namespace Name\Space { ... }
                // Namespace names can contain reserved keywords as parts
                let mut name_parts = Vec::new();
                if matches!(self.peek(), TokenKind::Identifier(_))
                    || self.is_semi_reserved_keyword()
                    || matches!(
                        self.peek(),
                        TokenKind::Fn
                            | TokenKind::Match
                            | TokenKind::Null
                            | TokenKind::True
                            | TokenKind::False
                    )
                {
                    loop {
                        match self.peek().clone() {
                            TokenKind::Identifier(part) => {
                                self.advance();
                                name_parts.push(part);
                            }
                            _ if self.is_semi_reserved_keyword()
                                || matches!(
                                    self.peek(),
                                    TokenKind::Fn
                                        | TokenKind::Match
                                        | TokenKind::Null
                                        | TokenKind::True
                                        | TokenKind::False
                                ) =>
                            {
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
                        // A leading separator is a fully qualified name.
                        if matches!(self.peek(), TokenKind::Backslash) {
                            let mut fq = String::new();
                            let mut i = self.pos;
                            while let Some(t) = self.tokens.get(i) {
                                match &t.kind {
                                    TokenKind::Backslash => fq.push('\\'),
                                    TokenKind::Identifier(p) => {
                                        fq.push_str(&String::from_utf8_lossy(p))
                                    }
                                    _ => break,
                                }
                                i += 1;
                            }
                            return Err(ParseError {
                                message: format!(
                                    "syntax error, unexpected fully qualified name \"{}\", expecting identifier or namespaced name or \"function\" or \"const\"",
                                    fq
                                ),
                                span: self.span(),
                            });
                        }
                        // Check for `function` or `const` per-item kind
                        // (only in a mixed group use).
                        let mixed = matches!(default_kind, UseKind::Normal);
                        let item_kind = match self.peek() {
                            TokenKind::Function | TokenKind::Const if !mixed => {
                                return Err(ParseError {
                                    message: format!(
                                        "syntax error, unexpected {}, expecting \"}}\"",
                                        self.peek().to_php_unexpected()
                                    ),
                                    span: self.span(),
                                });
                            }
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
                let mut exprs: Vec<Expr> = Vec::new();
                let mut dynamic = false;
                loop {
                    if let TokenKind::Variable(name) = self.peek().clone() {
                        let vspan = self.span();
                        self.advance();
                        exprs.push(Expr {
                            kind: ExprKind::Variable(name.clone()),
                            span: vspan,
                        });
                        vars.push(name);
                    } else if matches!(
                        self.peek(),
                        TokenKind::DollarOpenBrace
                            | TokenKind::VariableVariable(_)
                            | TokenKind::Dollar
                    ) {
                        // global ${expr}; / global $$name;
                        exprs.push(self.parse_simple_variable()?);
                        dynamic = true;
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
                if !matches!(self.peek(), TokenKind::Semicolon | TokenKind::CloseTag) {
                    return Err(ParseError {
                        message: format!(
                            "syntax error, unexpected {}, expecting \",\" or \";\"",
                            self.peek().to_php_unexpected()
                        ),
                        span: self.span(),
                    });
                }
                self.expect_semicolon()?;
                Ok(Statement {
                    kind: if dynamic {
                        StmtKind::GlobalDynamic(exprs)
                    } else {
                        StmtKind::Global(vars)
                    },
                    span,
                })
            }
            TokenKind::Unset => {
                self.advance();
                self.expect(&TokenKind::OpenParen)?;
                let mut exprs = vec![self.parse_expression()?];
                while self.eat(&TokenKind::Comma) {
                    if matches!(self.peek(), TokenKind::CloseParen) {
                        break;
                    }
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
                        message: format!(
                            "syntax error, unexpected {}, expecting identifier",
                            self.peek().to_php_unexpected()
                        ),
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
                            && matches!(self.peek_at(3), TokenKind::Semicolon)
                        {
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
                // PHP attaches a doc-comment that appears directly before an
                // assignment of a closure/arrow function to the closure/fn
                // itself (so reflection getDocComment() works on the closure).
                // Detect `/** doc */ $x = function/fn/static function/static fn ...`
                // and stash the doc for parse_primary to pick up.
                self.pending_closure_doc = None;
                if let Some(doc) = self.peek_doc_comment().cloned() {
                    let is_closure_assign = match self.peek() {
                        TokenKind::Variable(_) => {
                            // Look ahead for Assign followed by Function/Fn/Static(Function|Fn)
                            let p = self.pos + 1;
                            let after = self.tokens.get(p).map(|t| &t.kind);
                            let then = self.tokens.get(p + 1).map(|t| &t.kind);
                            let then2 = self.tokens.get(p + 2).map(|t| &t.kind);
                            matches!(after, Some(TokenKind::Assign))
                                && (matches!(then, Some(TokenKind::Function) | Some(TokenKind::Fn))
                                    || (matches!(then, Some(TokenKind::Static))
                                        && matches!(
                                            then2,
                                            Some(TokenKind::Function) | Some(TokenKind::Fn)
                                        )))
                        }
                        _ => false,
                    };
                    if is_closure_assign {
                        // Consume the doc-comment and stash it for the closure.
                        let _ = self.take_doc_comment();
                        self.pending_closure_doc = Some(doc);
                    }
                }
                // Expression statement
                let expr = self.parse_expression()?;
                self.expect_semicolon()?;
                self.pending_closure_doc = None;
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

    /// Like parse_block but also returns the line number of the closing brace.
    fn parse_block_with_end(&mut self) -> ParseResult<(Vec<Statement>, u32)> {
        self.expect(&TokenKind::OpenBrace)?;
        let mut stmts = Vec::new();
        while !matches!(self.peek(), TokenKind::CloseBrace | TokenKind::Eof) {
            stmts.push(self.parse_statement()?);
        }
        let end_line = self.span().line;
        self.expect(&TokenKind::CloseBrace)?;
        Ok((stmts, end_line))
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
                self.expect_case_separator()?;
                Some(v)
            } else if self.eat(&TokenKind::Default) {
                self.expect_case_separator()?;
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

    fn parse_const_with_attrs(&mut self, attributes: Vec<Attribute>) -> ParseResult<Statement> {
        let span = self.span();
        self.advance(); // const
        // Parse one or more name=value pairs separated by commas
        let mut directives: Vec<(Vec<u8>, Expr)> = Vec::new();
        let has_attrs = !attributes.is_empty();
        loop {
            let name = match self.peek().clone() {
                TokenKind::Identifier(name) => {
                    self.advance();
                    name
                }
                // `enum` is only a keyword before a class-like name, and
                // null/true/false are plain T_STRINGs.
                TokenKind::Enum | TokenKind::Null | TokenKind::True | TokenKind::False => {
                    let kw = self.keyword_to_identifier();
                    self.advance();
                    kw
                }
                // const_decl takes a T_STRING: reserved words are rejected.
                _ => {
                    return Err(ParseError {
                        message: format!(
                            "syntax error, unexpected {}, expecting identifier",
                            self.peek().to_php_unexpected()
                        ),
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
        let _ = has_attrs; // for future use
        self.expect_semicolon()?;
        Ok(Statement {
            kind: StmtKind::Declare {
                directives,
                body: None,
                attributes,
            },
            span,
        })
    }

    fn parse_function_decl_with_attrs(
        &mut self,
        attributes: Vec<Attribute>,
    ) -> ParseResult<Statement> {
        let span = self.span();
        self.advance(); // function
        // Optional & for return-by-reference
        let returns_by_ref = self.eat(&TokenKind::Ampersand);
        let name = match self.peek().clone() {
            TokenKind::Identifier(name) => {
                self.advance();
                name
            }
            // function_name: T_STRING | T_READONLY (`enum` is contextual).
            TokenKind::Readonly
            | TokenKind::Enum
            | TokenKind::Null
            | TokenKind::True
            | TokenKind::False => {
                let kw = self.keyword_to_identifier();
                self.advance();
                kw
            }
            _ => {
                return Err(ParseError {
                    message: format!(
                        "syntax error, unexpected {}, expecting \"(\"",
                        self.peek().to_php_unexpected()
                    ),
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

        let (body, end_line) = self.parse_block_with_end()?;

        Ok(Statement {
            kind: StmtKind::FunctionDecl {
                name,
                params,
                return_type,
                body,
                is_static: false,
                attributes,
                end_line,
                doc_comment: None,
                returns_by_ref,
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
            // Capture doc comment immediately before this parameter (attached
            // to the first token of the parameter, whichever comes first).
            let param_doc_comment = self.take_doc_comment();
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
                        if matches!(self.peek(), TokenKind::OpenParen)
                            && self.lookahead_is_set_modifier()
                        {
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
            self.last_set_hook_type = None;
            let (
                param_get_hook,
                param_set_hook,
                param_get_hook_final,
                param_set_hook_final,
                param_get_hook_abstract,
                param_set_hook_abstract,
            ) = if matches!(self.peek(), TokenKind::OpenBrace) {
                self.parse_property_hooks(&name, type_hint.as_ref())?
            } else {
                (None, None, false, false, false, false)
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
                get_hook_final: param_get_hook_final,
                set_hook_final: param_set_hook_final,
                get_hook_abstract: param_get_hook_abstract,
                set_hook_abstract: param_set_hook_abstract,
                doc_comment: param_doc_comment,
                set_hook_type: self.last_set_hook_type.take(),
                hook_attrs: std::mem::take(&mut self.last_hook_attrs),
                get_hook_by_ref: std::mem::take(&mut self.last_get_hook_by_ref),
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
                            message: format!(
                                "Type {} cannot be part of an intersection type",
                                err_name
                            ),
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
                        message: format!(
                            "Type {} cannot be part of an intersection type",
                            err_name
                        ),
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
                    b"int" | b"float" | b"string" | b"bool" | b"array" | b"null" | b"void"
                    | b"never" | b"object" | b"mixed" | b"callable" | b"false" | b"true"
                    | b"static" | b"iterable" => {
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
            while let TokenKind::Identifier(part) = self.peek().clone() {
                self.advance();
                if name.len() > 1 {
                    name.push(b'\\');
                }
                name.extend_from_slice(&part);
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

    fn parse_class_decl_with_attrs(
        &mut self,
        attributes: Vec<Attribute>,
    ) -> ParseResult<Statement> {
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
                span,
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
            // Capture the backing type (validated by the compiler: int or
            // string; anything else is reported as written).
            fn render(t: &TypeHint) -> Vec<u8> {
                match t {
                    TypeHint::Simple(n) => n.clone(),
                    TypeHint::Nullable(i) => [b"?".as_slice(), &render(i)].concat(),
                    TypeHint::Union(ps) => {
                        // zend_type_to_string() order: classes, then builtins.
                        let rank = |t: &TypeHint| match t {
                            TypeHint::Simple(n) => {
                                let order: [&[u8]; 13] = [
                                    b"static",
                                    b"callable",
                                    b"object",
                                    b"array",
                                    b"string",
                                    b"int",
                                    b"float",
                                    b"bool",
                                    b"false",
                                    b"true",
                                    b"void",
                                    b"never",
                                    b"null",
                                ];
                                order
                                    .iter()
                                    .position(|o| n.eq_ignore_ascii_case(o))
                                    .map_or(0, |i| i + 1)
                            }
                            _ => 0,
                        };
                        let mut ps: Vec<&TypeHint> = ps.iter().collect();
                        ps.sort_by_key(|t| rank(t));
                        ps.into_iter().map(render).collect::<Vec<_>>().join(&b'|')
                    }
                    TypeHint::Intersection(ps) => {
                        ps.iter().map(render).collect::<Vec<_>>().join(&b'&')
                    }
                }
            }
            let backing = render(&self.parse_type_hint()?);
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
        if matches!(self.peek(), TokenKind::Implements) {
            // Traits cannot implement interfaces — PHP raises a parse error here.
            if modifiers.is_trait {
                return Err(ParseError {
                    message: "syntax error, unexpected token \"implements\", expecting \"{\""
                        .into(),
                    span: self.span(),
                });
            }
            self.advance(); // eat implements
            loop {
                implements.push(self.parse_class_name_ref()?);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }

        self.expect(&TokenKind::OpenBrace)?;
        let mut body = Vec::new();
        self.class_name_stack.push(name.clone());
        self.is_interface_stack.push(modifiers.is_interface);
        let mut parse_err: Option<ParseError> = None;
        while !matches!(self.peek(), TokenKind::CloseBrace | TokenKind::Eof) {
            match self.parse_class_members() {
                Ok(members) => body.extend(members),
                Err(e) => {
                    parse_err = Some(e);
                    break;
                }
            }
        }
        self.class_name_stack.pop();
        self.is_interface_stack.pop();
        if let Some(e) = parse_err {
            return Err(e);
        }
        let end_line = self.span().line;
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
                end_line,
                doc_comment: None,
            },
            span,
        })
    }

    fn parse_class_body(&mut self) -> ParseResult<(Vec<ClassMember>, u32)> {
        self.expect(&TokenKind::OpenBrace)?;
        let mut body = Vec::new();
        while !matches!(self.peek(), TokenKind::CloseBrace | TokenKind::Eof) {
            body.extend(self.parse_class_members()?);
        }
        let end_line = self.span().line;
        self.expect(&TokenKind::CloseBrace)?;
        Ok((body, end_line))
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
        // Capture doc comment before attributes/modifiers
        let member_doc_comment = self.take_doc_comment();
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
                                    message: "Multiple access type modifiers are not allowed"
                                        .into(),
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
                Ok(ClassMember::EnumCase {
                    name,
                    value,
                    attributes: member_attributes,
                    doc_comment: member_doc_comment,
                })
            }
            TokenKind::Use => {
                // trait use
                self.advance();
                let mut traits = Vec::new();
                loop {
                    if matches!(self.peek(), TokenKind::Static) {
                        return Err(self.static_trait_name_error());
                    }
                    if matches!(self.peek(), TokenKind::Identifier(_) | TokenKind::Backslash) {
                        let span = self.current().span;
                        let n = self.parse_class_name_ref()?;
                        if n.eq_ignore_ascii_case(b"self") || n.eq_ignore_ascii_case(b"parent") {
                            return Err(ParseError {
                                message: format!(
                                    "Cannot use \"{}\" as trait name, as it is reserved",
                                    String::from_utf8_lossy(&n)
                                ),
                                span,
                            });
                        }
                        traits.push(n);
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
                            TokenKind::Static
                                if matches!(self.peek_at(1), TokenKind::DoubleColon) =>
                            {
                                return Err(self.static_trait_name_error());
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
                                    if matches!(self.peek(), TokenKind::Static) {
                                        return Err(self.static_trait_name_error());
                                    }
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
                    return Err(ParseError {
                        message: "Cannot use the readonly modifier on a method".into(),
                        span: self.span(),
                    });
                }
                let method_line = self.span().line;
                self.advance();
                // Optional & for return-by-reference
                let method_by_ref = self.eat(&TokenKind::Ampersand);
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

                let method_end_line: u32;
                let body = if is_abstract {
                    if matches!(self.peek(), TokenKind::Semicolon) {
                        method_end_line = self.span().line;
                        self.expect_semicolon()?;
                        None
                    } else if matches!(self.peek(), TokenKind::OpenBrace) {
                        // Abstract method with body - parse it but it will be an error
                        let (b, el) = self.parse_block_with_end()?;
                        method_end_line = el;
                        Some(b)
                    } else {
                        method_end_line = self.span().line;
                        self.expect_semicolon()?;
                        None
                    }
                } else if matches!(self.peek(), TokenKind::Semicolon) {
                    method_end_line = self.span().line;
                    self.expect_semicolon()?;
                    None
                } else {
                    let (b, el) = self.parse_block_with_end()?;
                    method_end_line = el;
                    Some(b)
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
                    end_line: method_end_line,
                    attributes: member_attributes,
                    doc_comment: member_doc_comment,
                    returns_by_ref: method_by_ref,
                })
            }
            TokenKind::Const => {
                if is_readonly {
                    return Err(ParseError {
                        message: "Cannot use the readonly modifier on a class constant".into(),
                        span: self.span(),
                    });
                }
                if is_static {
                    return Err(ParseError {
                        message: "Cannot use the static modifier on a class constant".into(),
                        span: self.span(),
                    });
                }
                if is_abstract {
                    return Err(ParseError {
                        message: "Cannot use the abstract modifier on a class constant".into(),
                        span: self.span(),
                    });
                }
                self.advance();

                // Detect whether the next thing is a type hint or directly the
                // constant name. Save state to allow rollback.
                let saved_pos = self.pos;
                let mut const_type: Option<TypeHint> = None;
                let mut name: Option<Vec<u8>> = None;

                // Capture doc comment associated with the post-const token. If
                // the source is `public const /** doc */ TYPE NAME`, the doc
                // attaches to the TYPE token here. Also handle the common case
                // where the doc appears on the previous line and attached to
                // either `const` or `public`.
                let pre_type_doc = self.take_doc_comment();

                // A type hint definitely starts with ?, (, or is followed by
                // | / & / identifier-after-identifier. Try parsing a type hint
                // speculatively when we see those leading tokens.
                let starts_with_type_token =
                    matches!(self.peek(), TokenKind::QuestionMark | TokenKind::OpenParen);

                if starts_with_type_token {
                    // Must be a type hint
                    let th = self.parse_type_hint()?;
                    const_type = Some(th);
                } else if matches!(self.peek(), TokenKind::Identifier(_))
                    || self.is_semi_reserved_keyword()
                {
                    // Could be type hint OR constant name. Look ahead.
                    // Peek past potential identifier and check next token:
                    // - If next is `=` -> identifier was the constant name.
                    // - If next is `|` or `&` -> identifier is start of union/intersection type.
                    // - If next is identifier (semi-reserved or not) -> identifier was a type.
                    let next_kind = self.peek_at(1).clone();
                    let is_type_followed_by_name = matches!(
                        next_kind,
                        TokenKind::Pipe | TokenKind::Ampersand | TokenKind::Identifier(_)
                    ) || matches!(next_kind, _ if Self::is_kind_semi_reserved(&next_kind));
                    if is_type_followed_by_name {
                        // It's a type hint.
                        let th = self.parse_type_hint()?;
                        const_type = Some(th);
                    } else {
                        // It's the constant name. Don't parse a type hint.
                    }
                }

                // Check for a doc comment immediately preceding the const name
                // (e.g. `public const bool /** doc */ X = ...`).
                let pre_name_doc = self.take_doc_comment();
                // Priority: doc immediately before the name, then before the
                // type, then before the entire declaration.
                let const_doc = pre_name_doc.or(pre_type_doc).or(member_doc_comment.clone());

                // Now parse the constant name (if not already)
                if name.is_none() && matches!(self.peek(), TokenKind::Class) {
                    return Err(ParseError {
                        message: "A class constant must not be called 'class'; it is reserved for class name fetching".into(),
                        span: self.span(),
                    });
                }
                if name.is_none() {
                    name = Some(match self.peek().clone() {
                        TokenKind::Identifier(n) => {
                            self.advance();
                            n
                        }
                        _ if self.is_semi_reserved_keyword() => {
                            let kw = self.keyword_to_identifier();
                            self.advance();
                            kw
                        }
                        _ => {
                            // Rollback to saved_pos so error messages make sense
                            self.pos = saved_pos;
                            return Err(ParseError {
                                message: "expected constant name".into(),
                                span: self.span(),
                            });
                        }
                    });
                }
                let name = name.unwrap();

                self.expect(&TokenKind::Assign)?;
                let value = self.parse_expression()?;

                // Handle comma-separated constants: const TYPE A = 1, B = 2;
                self.pending_extra_constants.clear();
                while self.eat(&TokenKind::Comma) {
                    let extra_name = match self.peek().clone() {
                        TokenKind::Identifier(n) => {
                            self.advance();
                            n
                        }
                        _ if self.is_semi_reserved_keyword() => {
                            let kw = self.keyword_to_identifier();
                            self.advance();
                            kw
                        }
                        _ => break,
                    };
                    self.expect(&TokenKind::Assign)?;
                    let extra_value = self.parse_expression()?;
                    self.pending_extra_constants
                        .push(ClassMember::ClassConstant {
                            name: extra_name,
                            value: extra_value,
                            visibility,
                            is_final,
                            attributes: member_attributes.clone(),
                            type_hint: const_type.clone(),
                            doc_comment: const_doc.clone(),
                        });
                }

                self.expect_semicolon()?;
                Ok(ClassMember::ClassConstant {
                    name,
                    value,
                    visibility,
                    is_final,
                    attributes: member_attributes,
                    type_hint: const_type,
                    doc_comment: const_doc,
                })
            }
            TokenKind::Variable(_) => {
                // Property (possibly with type hint already consumed as a modifier)
                let prop_line = self.span().line;
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
                self.last_set_hook_type = None;
                let (
                    get_hook,
                    set_hook,
                    get_hook_final,
                    set_hook_final,
                    get_hook_abstract,
                    set_hook_abstract,
                ) = if matches!(self.peek(), TokenKind::OpenBrace) {
                    self.parse_property_hooks(&name, None)?
                } else {
                    // Parse comma-separated additional properties
                    self.pending_extra_properties.clear();
                    while self.eat(&TokenKind::Comma) {
                        // PHP captures a doc-comment that appears immediately
                        // before each individual property in a comma list.
                        // Only the first property gets the outer member doc-
                        // comment; for subsequent ones, only an explicit
                        // doc-comment between commas applies.
                        let extra_doc = self.take_doc_comment();
                        if let TokenKind::Variable(extra_name) = self.peek().clone() {
                            let extra_line = self.span().line;
                            self.advance();
                            let extra_default = if self.eat(&TokenKind::Assign) {
                                Some(self.parse_expression()?)
                            } else {
                                None
                            };
                            self.pending_extra_properties.push(ClassMember::Property {
                                name: extra_name,
                                line: extra_line,
                                type_hint: None,
                                default: extra_default,
                                visibility,
                                set_visibility,
                                is_static,
                                is_readonly,
                                is_abstract: false,
                                is_final: false,
                                get_hook: None,
                                set_hook: None,
                                get_hook_final: false,
                                set_hook_final: false,
                                get_hook_abstract: false,
                                set_hook_abstract: false,
                                attributes: member_attributes.clone(),
                                doc_comment: extra_doc,
                                set_hook_type: None,
                                hook_attrs: HookAttrs::default(),
                                get_hook_by_ref: false,
                            });
                        } else {
                            return Err(ParseError {
                                message: format!(
                                    "syntax error, unexpected {}, expecting variable",
                                    self.peek().to_php_unexpected()
                                ),
                                span: self.span(),
                            });
                        }
                    }
                    self.expect_semicolon()?;
                    (None, None, false, false, false, false)
                };
                // In interfaces, properties cannot be explicitly abstract — all interface
                // members are implicitly abstract.
                if self.is_interface_stack.last().copied().unwrap_or(false) {
                    if is_abstract {
                        return Err(ParseError {
                            message: "Property in interface cannot be explicitly abstract. All interface members are implicitly abstract".into(),
                            span: self.span(),
                        });
                    }
                    if is_final {
                        return Err(ParseError {
                            message: "Property in interface cannot be final".into(),
                            span: self.span(),
                        });
                    }
                    if matches!(visibility, Visibility::Protected | Visibility::Private) {
                        return Err(ParseError {
                            message: "Property in interface cannot be protected or private".into(),
                            span: self.span(),
                        });
                    }
                }
                // Abstract properties must have hooks
                if is_abstract && get_hook.is_none() && set_hook.is_none() {
                    return Err(ParseError {
                        message: "Only hooked properties may be declared abstract".into(),
                        span: self.span(),
                    });
                }
                // Abstract property must have at least one abstract hook
                if is_abstract && !get_hook_abstract && !set_hook_abstract {
                    let class_name = self
                        .class_name_stack
                        .last()
                        .map(|n| String::from_utf8_lossy(n).into_owned())
                        .unwrap_or_else(|| "?".to_string());
                    return Err(ParseError {
                        message: format!(
                            "Abstract property {}::${} must specify at least one abstract hook",
                            class_name,
                            String::from_utf8_lossy(&name),
                        ),
                        span: self.span(),
                    });
                }
                if is_static && (get_hook.is_some() || set_hook.is_some()) {
                    return Err(ParseError {
                        message: "Cannot declare hooks for static property".into(),
                        span: self.span(),
                    });
                }
                Ok(ClassMember::Property {
                    name,
                    line: prop_line,
                    type_hint: None,
                    default,
                    visibility,
                    set_visibility,
                    is_static,
                    is_readonly,
                    is_abstract,
                    is_final,
                    get_hook,
                    set_hook,
                    get_hook_final,
                    set_hook_final,
                    get_hook_abstract,
                    set_hook_abstract,
                    attributes: member_attributes.clone(),
                    doc_comment: member_doc_comment,
                    set_hook_type: self.last_set_hook_type.take(),
                    hook_attrs: std::mem::take(&mut self.last_hook_attrs),
                    get_hook_by_ref: std::mem::take(&mut self.last_get_hook_by_ref),
                })
            }
            _ => {
                // Might be a typed property: type $name
                let type_hint = self.parse_type_hint()?;
                let prop_line = self.span().line;
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
                self.last_set_hook_type = None;
                let (
                    get_hook,
                    set_hook,
                    get_hook_final,
                    set_hook_final,
                    get_hook_abstract,
                    set_hook_abstract,
                ) = if matches!(self.peek(), TokenKind::OpenBrace) {
                    self.parse_property_hooks(&name, Some(&type_hint))?
                } else {
                    // Parse comma-separated additional properties (they share the same type)
                    while self.eat(&TokenKind::Comma) {
                        // See note on the untyped path: each comma-listed
                        // property gets its own doc-comment if one appears
                        // immediately before it; otherwise None.
                        let extra_doc = self.take_doc_comment();
                        if let TokenKind::Variable(extra_name) = self.peek().clone() {
                            let extra_line = self.span().line;
                            self.advance();
                            let extra_default = if self.eat(&TokenKind::Assign) {
                                Some(self.parse_expression()?)
                            } else {
                                None
                            };
                            self.pending_extra_properties.push(ClassMember::Property {
                                name: extra_name,
                                line: extra_line,
                                type_hint: Some(type_hint.clone()),
                                default: extra_default,
                                visibility,
                                set_visibility,
                                is_static,
                                is_readonly,
                                is_abstract: false,
                                is_final: false,
                                get_hook: None,
                                set_hook: None,
                                get_hook_final: false,
                                set_hook_final: false,
                                get_hook_abstract: false,
                                set_hook_abstract: false,
                                attributes: member_attributes.clone(),
                                doc_comment: extra_doc,
                                set_hook_type: None,
                                hook_attrs: HookAttrs::default(),
                                get_hook_by_ref: false,
                            });
                        } else {
                            return Err(ParseError {
                                message: format!(
                                    "syntax error, unexpected {}, expecting variable",
                                    self.peek().to_php_unexpected()
                                ),
                                span: self.span(),
                            });
                        }
                    }
                    self.expect_semicolon()?;
                    (None, None, false, false, false, false)
                };
                // In interfaces, properties cannot be explicitly abstract — all interface
                // members are implicitly abstract.
                if self.is_interface_stack.last().copied().unwrap_or(false) {
                    if is_abstract {
                        return Err(ParseError {
                            message: "Property in interface cannot be explicitly abstract. All interface members are implicitly abstract".into(),
                            span: self.span(),
                        });
                    }
                    if is_final {
                        return Err(ParseError {
                            message: "Property in interface cannot be final".into(),
                            span: self.span(),
                        });
                    }
                    if matches!(visibility, Visibility::Protected | Visibility::Private) {
                        return Err(ParseError {
                            message: "Property in interface cannot be protected or private".into(),
                            span: self.span(),
                        });
                    }
                }
                // Abstract properties must have hooks
                if is_abstract && get_hook.is_none() && set_hook.is_none() {
                    return Err(ParseError {
                        message: "Only hooked properties may be declared abstract".into(),
                        span: self.span(),
                    });
                }
                // Abstract property must have at least one abstract hook
                if is_abstract && !get_hook_abstract && !set_hook_abstract {
                    let class_name = self
                        .class_name_stack
                        .last()
                        .map(|n| String::from_utf8_lossy(n).into_owned())
                        .unwrap_or_else(|| "?".to_string());
                    return Err(ParseError {
                        message: format!(
                            "Abstract property {}::${} must specify at least one abstract hook",
                            class_name,
                            String::from_utf8_lossy(&name),
                        ),
                        span: self.span(),
                    });
                }
                if is_static && (get_hook.is_some() || set_hook.is_some()) {
                    return Err(ParseError {
                        message: "Cannot declare hooks for static property".into(),
                        span: self.span(),
                    });
                }
                Ok(ClassMember::Property {
                    name,
                    line: prop_line,
                    type_hint: Some(type_hint),
                    default,
                    visibility,
                    set_visibility,
                    is_static,
                    is_readonly,
                    is_abstract,
                    is_final,
                    get_hook,
                    set_hook,
                    get_hook_final,
                    set_hook_final,
                    get_hook_abstract,
                    set_hook_abstract,
                    attributes: member_attributes,
                    doc_comment: member_doc_comment,
                    set_hook_type: self.last_set_hook_type.take(),
                    hook_attrs: std::mem::take(&mut self.last_hook_attrs),
                    get_hook_by_ref: std::mem::take(&mut self.last_get_hook_by_ref),
                })
            }
        }
    }

    /// Parse property hooks: { get { ... } set($value) { ... } }
    /// Returns (get_hook, set_hook)
    fn parse_property_hooks(
        &mut self,
        prop_name: &[u8],
        prop_type_hint: Option<&TypeHint>,
    ) -> ParseResult<(
        Option<Vec<Statement>>,
        Option<(Vec<u8>, Vec<Statement>)>,
        bool,
        bool,
        bool,
        bool,
    )> {
        self.expect(&TokenKind::OpenBrace)?;
        let mut get_hook = None;
        let mut set_hook = None;
        let mut get_hook_final = false;
        let mut set_hook_final = false;
        let mut get_hook_abstract = false;
        self.last_get_hook_by_ref = false;
        self.last_hook_attrs = HookAttrs::default();
        let mut set_hook_abstract = false;

        while !matches!(self.peek(), TokenKind::CloseBrace | TokenKind::Eof) {
            // Skip optional attributes and modifiers: #[...] final, public, protected, private, &
            let mut hook_attrs = Vec::new();
            let attr_span = self.span();
            while matches!(self.peek(), TokenKind::AttributeOpen) {
                // Parse and discard attributes for now
                hook_attrs.extend(self.parse_attributes()?);
            }
            // zend_compile_func_decl(): #[\NoDiscard] cannot apply to hooks
            // (unless its validation is delayed).
            let is_attr = |a: &Attribute, n: &[u8]| {
                a.name
                    .last()
                    .is_some_and(|l| l.strip_prefix(b"\\").unwrap_or(l).eq_ignore_ascii_case(n))
            };
            if hook_attrs.iter().any(|a| is_attr(a, b"NoDiscard"))
                && !hook_attrs
                    .iter()
                    .any(|a| is_attr(a, b"DelayedTargetValidation"))
            {
                return Err(ParseError {
                    message: "#[\\NoDiscard] is not supported for property hooks".into(),
                    span: attr_span,
                });
            }
            let mut hook_is_final = false;
            let mut hook_is_static = false;
            let mut hook_is_private = false;
            let mut visibility_modifier: Option<&'static str> = None;
            let mut hook_is_by_ref = false;
            loop {
                match self.peek() {
                    TokenKind::Final => {
                        hook_is_final = true;
                        self.advance();
                    }
                    TokenKind::Static => {
                        hook_is_static = true;
                        self.advance();
                    }
                    TokenKind::Private => {
                        hook_is_private = true;
                        visibility_modifier = Some("private");
                        self.advance();
                    }
                    TokenKind::Public => {
                        visibility_modifier = Some("public");
                        self.advance();
                    }
                    TokenKind::Protected => {
                        visibility_modifier = Some("protected");
                        self.advance();
                    }
                    TokenKind::Ampersand => {
                        hook_is_by_ref = true;
                        self.advance();
                    }
                    _ => break,
                }
            }
            // Reject invalid combinations
            if hook_is_static {
                return Err(ParseError {
                    message: "Cannot use the static modifier on a property hook".into(),
                    span: self.span(),
                });
            }
            if let Some(vis) = visibility_modifier {
                return Err(ParseError {
                    message: format!("Cannot use the {} modifier on a property hook", vis),
                    span: self.span(),
                });
            }
            let _ = hook_is_private;
            if hook_is_by_ref
                && matches!(self.peek(), TokenKind::Identifier(n) if n.eq_ignore_ascii_case(b"get"))
            {
                self.last_get_hook_by_ref = true;
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
                if get_hook.is_some() {
                    return Err(ParseError {
                        message: "Cannot redeclare property hook \"get\"".into(),
                        span: self.span(),
                    });
                }
                if hook_is_final {
                    get_hook_final = true;
                }
                self.last_hook_attrs.get = std::mem::take(&mut hook_attrs);
                self.advance(); // consume 'get'
                if matches!(self.peek(), TokenKind::OpenParen) {
                    let class_name = self
                        .class_name_stack
                        .last()
                        .map(|n| String::from_utf8_lossy(n).into_owned())
                        .unwrap_or_else(|| "?".to_string());
                    return Err(ParseError {
                        message: format!(
                            "get hook of property {}::${} must not have a parameter list",
                            class_name,
                            String::from_utf8_lossy(prop_name),
                        ),
                        span: self.span(),
                    });
                }
                if matches!(self.peek(), TokenKind::OpenBrace) {
                    // get { ... }
                    let body = self.parse_block()?;
                    get_hook = Some(body);
                } else if self.eat(&TokenKind::DoubleArrow) {
                    // get => expr;
                    self.last_hook_attrs.get_short = true;
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
                    get_hook_abstract = true;
                } else {
                    return Err(ParseError {
                        message: "expected '{' or '=>' after 'get' in property hook".into(),
                        span: self.span(),
                    });
                }
            } else if hook_name.eq_ignore_ascii_case(b"set") {
                if set_hook.is_some() {
                    return Err(ParseError {
                        message: "Cannot redeclare property hook \"set\"".into(),
                        span: self.span(),
                    });
                }
                if hook_is_final {
                    set_hook_final = true;
                }
                self.last_hook_attrs.set = std::mem::take(&mut hook_attrs);
                self.advance(); // consume 'set'
                // Optional parameter: set(Type $value) or set($value) or just set
                let had_paren = matches!(self.peek(), TokenKind::OpenParen);
                let (param_name, param_type_hint) = if self.eat(&TokenKind::OpenParen) {
                    // We need to handle: set($value), set(string $value), set(Type $value)
                    // Also accept attributes #[Attr] before the parameter.
                    while matches!(self.peek(), TokenKind::AttributeOpen) {
                        let a = self.parse_attributes()?;
                        self.last_hook_attrs.set_param.extend(a);
                    }
                    // Detect by-ref / variadic before or after type hint.
                    let mut got_by_ref = matches!(self.peek(), TokenKind::Ampersand);
                    let mut got_variadic = matches!(self.peek(), TokenKind::Ellipsis);
                    // If the next token is a variable, no type hint; else parse type hint.
                    let type_hint = if matches!(
                        self.peek(),
                        TokenKind::Variable(_)
                            | TokenKind::CloseParen
                            | TokenKind::Ampersand
                            | TokenKind::Ellipsis
                    ) {
                        None
                    } else {
                        Some(self.parse_type_hint()?)
                    };
                    if !got_by_ref && matches!(self.peek(), TokenKind::Ampersand) {
                        got_by_ref = true;
                        self.advance();
                    } else if got_by_ref {
                        self.advance();
                    }
                    if !got_variadic && matches!(self.peek(), TokenKind::Ellipsis) {
                        got_variadic = true;
                        self.advance();
                    } else if got_variadic {
                        self.advance();
                    }
                    let pname = match self.peek().clone() {
                        TokenKind::Variable(name) => {
                            self.advance();
                            name
                        }
                        TokenKind::CloseParen => b"value".to_vec(),
                        _ => {
                            return Err(ParseError {
                                message: "expected parameter name in set hook".into(),
                                span: self.span(),
                            });
                        }
                    };
                    if got_by_ref || got_variadic {
                        let class_name = self
                            .class_name_stack
                            .last()
                            .map(|n| String::from_utf8_lossy(n).into_owned())
                            .unwrap_or_else(|| "?".to_string());
                        let kind = if got_by_ref {
                            "pass-by-reference"
                        } else {
                            "variadic"
                        };
                        return Err(ParseError {
                            message: format!(
                                "Parameter ${} of set hook {}::${} must not be {}",
                                String::from_utf8_lossy(&pname),
                                class_name,
                                String::from_utf8_lossy(prop_name),
                                kind,
                            ),
                            span: self.span(),
                        });
                    }
                    // No default value allowed: `set($value = null)` → error
                    if matches!(self.peek(), TokenKind::Assign) {
                        let class_name = self
                            .class_name_stack
                            .last()
                            .map(|n| String::from_utf8_lossy(n).into_owned())
                            .unwrap_or_else(|| "?".to_string());
                        return Err(ParseError {
                            message: format!(
                                "Parameter ${} of set hook {}::${} must not have a default value",
                                String::from_utf8_lossy(&pname),
                                class_name,
                                String::from_utf8_lossy(prop_name),
                            ),
                            span: self.span(),
                        });
                    }
                    self.expect(&TokenKind::CloseParen)?;
                    (pname, type_hint)
                } else {
                    (b"value".to_vec(), None)
                };
                // Enforce set $value parameter type variance. PHP requires the set hook's
                // parameter type to be compatible (LSP contravariant) with the property type.
                // We only emit the error when the user explicitly wrote a parameter list
                // (so `set => expr;` and `set { ... }` without parens are not checked,
                // because in those forms the implicit $value inherits the property type).
                if had_paren {
                    let mismatch = match (prop_type_hint, &param_type_hint) {
                        (Some(_), None) => true,
                        (None, Some(_)) => true,
                        (Some(pt), Some(pht)) => !set_param_type_compatible(pt, pht),
                        (None, None) => false,
                    };
                    if mismatch {
                        let class_name = self
                            .class_name_stack
                            .last()
                            .map(|n| String::from_utf8_lossy(n).into_owned())
                            .unwrap_or_else(|| "?".to_string());
                        return Err(ParseError {
                            message: format!(
                                "Type of parameter ${} of hook {}::${}::set must be compatible with property type",
                                String::from_utf8_lossy(&param_name),
                                class_name,
                                String::from_utf8_lossy(prop_name),
                            ),
                            span: self.span(),
                        });
                    }
                }
                self.last_set_hook_type = param_type_hint.clone();
                if matches!(self.peek(), TokenKind::OpenBrace) {
                    // set { ... } or set($value) { ... }
                    let body = self.parse_block()?;
                    set_hook = Some((param_name, body));
                } else if self.eat(&TokenKind::DoubleArrow) {
                    // set => expr; is shorthand for set { $this->propname = expr; }
                    self.last_hook_attrs.set_short = true;
                    let span = self.span();
                    let expr = self.parse_expression()?;
                    self.expect_semicolon()?;
                    // Generate: $this->propname = expr;
                    let this_expr = Expr {
                        kind: ExprKind::Variable(b"this".to_vec()),
                        span,
                    };
                    let prop_ident = Expr {
                        kind: ExprKind::Identifier(prop_name.to_vec()),
                        span,
                    };
                    let prop_access = Expr {
                        kind: ExprKind::PropertyAccess {
                            object: Box::new(this_expr),
                            property: Box::new(prop_ident),
                            nullsafe: false,
                        },
                        span,
                    };
                    let assign = Expr {
                        kind: ExprKind::Assign {
                            target: Box::new(prop_access),
                            value: Box::new(expr),
                        },
                        span,
                    };
                    let assign_stmt = Statement {
                        kind: StmtKind::Expression(assign),
                        span,
                    };
                    set_hook = Some((param_name, vec![assign_stmt]));
                } else if self.eat(&TokenKind::Semicolon) {
                    // set; (abstract hook)
                    set_hook = Some((param_name, vec![]));
                    set_hook_abstract = true;
                } else {
                    return Err(ParseError {
                        message: "expected '{' or '=>' after 'set' in property hook".into(),
                        span: self.span(),
                    });
                }
            } else {
                let class_name = self
                    .class_name_stack
                    .last()
                    .map(|n| String::from_utf8_lossy(n).into_owned())
                    .unwrap_or_else(|| "?".to_string());
                return Err(ParseError {
                    message: format!(
                        "Unknown hook \"{}\" for property {}::${}, expected \"get\" or \"set\"",
                        String::from_utf8_lossy(&hook_name),
                        class_name,
                        String::from_utf8_lossy(prop_name)
                    ),
                    span: self.span(),
                });
            }
        }

        self.expect(&TokenKind::CloseBrace)?;
        if get_hook.is_none() && set_hook.is_none() {
            return Err(ParseError {
                message: "Property hook list must not be empty".into(),
                span: self.span(),
            });
        }
        Ok((
            get_hook,
            set_hook,
            get_hook_final,
            set_hook_final,
            get_hook_abstract,
            set_hook_abstract,
        ))
    }

    fn parse_try_catch(&mut self) -> ParseResult<Statement> {
        let span = self.span();
        self.advance(); // try
        let try_body = self.parse_block()?;

        let mut catches = Vec::new();
        loop {
            let catch_line = self.span().line;
            if !self.eat(&TokenKind::Catch) {
                break;
            }
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
                line: catch_line,
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
            kind: StmtKind::Declare {
                directives,
                body,
                attributes: Vec::new(),
            },
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
        let e = self.parse_assignment_inner()?;
        // `@$x .= v` is `@($x .= v)`: the silence operator does not stop the
        // compound assignment from targeting the variable.
        if let ExprKind::CompoundAssign { target, .. } = &e.kind {
            if matches!(&target.kind, ExprKind::Suppress(o) if Self::is_lvalue(o)) {
                let span = e.span;
                let ExprKind::CompoundAssign { op, target, value } = e.kind else {
                    unreachable!()
                };
                let ExprKind::Suppress(operand) = target.kind else {
                    unreachable!()
                };
                let inner = Expr {
                    span: operand.span.merge(value.span),
                    kind: ExprKind::CompoundAssign {
                        op,
                        target: operand,
                        value,
                    },
                };
                return Ok(Expr {
                    span,
                    kind: ExprKind::Suppress(Box::new(inner)),
                });
            }
        }
        Ok(e)
    }

    fn parse_assignment_inner(&mut self) -> ParseResult<Expr> {
        let left = self.parse_null_coalesce()?;

        let compound_op = match self.peek() {
            TokenKind::Assign => None,
            TokenKind::PlusAssign => Some(BinaryOp::Add),
            TokenKind::MinusAssign => Some(BinaryOp::Sub),
            TokenKind::StarAssign => Some(BinaryOp::Mul),
            TokenKind::SlashAssign => Some(BinaryOp::Div),
            TokenKind::PercentAssign => Some(BinaryOp::Mod),
            TokenKind::PowAssign => Some(BinaryOp::Pow),
            TokenKind::DotAssign => Some(BinaryOp::Concat),
            TokenKind::AmpersandAssign => Some(BinaryOp::BitwiseAnd),
            TokenKind::PipeAssign => Some(BinaryOp::BitwiseOr),
            TokenKind::CaretAssign => Some(BinaryOp::BitwiseXor),
            TokenKind::ShiftLeftAssign => Some(BinaryOp::ShiftLeft),
            TokenKind::ShiftRightAssign => Some(BinaryOp::ShiftRight),
            // The target is evaluated once (zend_compile_assign_coalesce).
            TokenKind::NullCoalesceAssign => Some(BinaryOp::Coalesce),
            _ => return Ok(left),
        };
        let assign_span = self.span();
        self.advance();
        let by_ref = compound_op.is_none() && self.eat(&TokenKind::Ampersand);
        let right = self.parse_assignment()?;
        let after_span = self.span();
        // In PHP's grammar an assignment is `variable '=' expr`: the operator
        // always binds to the variable right before it, however tightly the
        // operators to its left bind. `$a + !$b = $c` is `$a + !($b = $c)`,
        // `$s ?? $s = ''` is `$s ?? ($s = '')`.
        Self::graft_assignment(left, move |target| {
            // A class constant is not writable: `C::X = v` is a syntax error.
            if compound_op.is_none()
                && !by_ref
                && matches!(target.kind, ExprKind::ClassConstAccess { .. })
            {
                return Err(ParseError {
                    message: "syntax error, unexpected token \"=\"".into(),
                    span: assign_span,
                });
            }
            // Nor can a reference to one be taken (`$a = &C::X`).
            if by_ref && matches!(right.kind, ExprKind::ClassConstAccess { .. }) {
                return Err(ParseError {
                    message: "syntax error, unexpected token \";\", expecting \"->\" or \"?->\" or \"{\" or \"[\"".into(),
                    span: after_span,
                });
            }
            let span = target.span.merge(right.span);
            let target = Box::new(target);
            let value = Box::new(right);
            let kind = match compound_op {
                None if by_ref => ExprKind::AssignRef { target, value },
                None => ExprKind::Assign { target, value },
                Some(op) => ExprKind::CompoundAssign { op, target, value },
            };
            Ok(Expr { span, kind })
        })
    }

    /// Attach an assignment to the rightmost operand of `left` that the
    /// assignment operator directly follows (see parse_assignment_inner).
    fn graft_assignment<F>(left: Expr, build: F) -> ParseResult<Expr>
    where
        F: FnOnce(Expr) -> ParseResult<Expr>,
    {
        if Self::is_lvalue(&left) {
            return build(left);
        }
        let span = left.span;
        Ok(match left.kind {
            ExprKind::BinaryOp { op, left: l, right } => {
                let right = Self::graft_assignment(*right, build)?;
                Expr {
                    span: span.merge(right.span),
                    kind: ExprKind::BinaryOp {
                        op,
                        left: l,
                        right: Box::new(right),
                    },
                }
            }
            ExprKind::UnaryOp {
                op,
                operand,
                prefix: true,
            } if !matches!(op, UnaryOp::PreIncrement | UnaryOp::PreDecrement) => {
                let operand = Self::graft_assignment(*operand, build)?;
                Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::UnaryOp {
                        op,
                        operand: Box::new(operand),
                        prefix: true,
                    },
                }
            }
            ExprKind::Cast(ty, operand) => {
                let operand = Self::graft_assignment(*operand, build)?;
                Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::Cast(ty, Box::new(operand)),
                }
            }
            ExprKind::Suppress(operand) => {
                let operand = Self::graft_assignment(*operand, build)?;
                Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::Suppress(Box::new(operand)),
                }
            }
            ExprKind::NullCoalesce { left: l, right } => {
                let right = Self::graft_assignment(*right, build)?;
                Expr {
                    span: span.merge(right.span),
                    kind: ExprKind::NullCoalesce {
                        left: l,
                        right: Box::new(right),
                    },
                }
            }
            ExprKind::Ternary {
                condition,
                if_true,
                if_false,
            } => {
                let if_false = Self::graft_assignment(*if_false, build)?;
                Expr {
                    span: span.merge(if_false.span),
                    kind: ExprKind::Ternary {
                        condition,
                        if_true,
                        if_false: Box::new(if_false),
                    },
                }
            }
            kind => build(Expr { span, kind })?,
        })
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
            let false_start = self.pos;
            let if_false = self.parse_ternary()?;
            // PHP's grammar is left-associative and rejects unparenthesized
            // nesting (except `a ?: b ?: c`). We parse right-associatively,
            // so check whether the false branch is a bare nested ternary.
            if let ExprKind::Ternary {
                if_true: inner_true,
                ..
            } = &if_false.kind
            {
                let parenthesized = self.tokens_parenthesized(false_start, self.pos);
                if !parenthesized {
                    let outer_short = if_true.is_none();
                    let inner_short = inner_true.is_none();
                    let msg = match (outer_short, inner_short) {
                        (false, false) => Some(
                            "Unparenthesized `a ? b : c ? d : e` is not supported. Use either `(a ? b : c) ? d : e` or `a ? b : (c ? d : e)`",
                        ),
                        (true, false) => Some(
                            "Unparenthesized `a ?: b ? c : d` is not supported. Use either `(a ?: b) ? c : d` or `a ?: (b ? c : d)`",
                        ),
                        (false, true) => Some(
                            "Unparenthesized `a ? b : c ?: d` is not supported. Use either `(a ? b : c) ?: d` or `a ? b : (c ?: d)`",
                        ),
                        (true, true) => None,
                    };
                    if let Some(m) = msg {
                        return Err(ParseError {
                            message: m.to_string(),
                            span: cond.span,
                        });
                    }
                }
            }
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
        let mut left = self.parse_equality()?;
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

    /// Parse pipe operator (|>), left-associative: it binds tighter than
    /// the comparisons and looser than `.` (zend_language_parser.y).
    fn parse_pipe(&mut self) -> ParseResult<Expr> {
        let mut left = self.parse_concat()?;
        while matches!(self.peek(), TokenKind::PipeGreater) {
            self.advance();
            let right = self.parse_concat()?;
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
        let mut left = self.parse_pipe()?;
        loop {
            let op = match self.peek() {
                TokenKind::Less => BinaryOp::Less,
                TokenKind::Greater => BinaryOp::Greater,
                TokenKind::LessEqual => BinaryOp::LessEqual,
                TokenKind::GreaterEqual => BinaryOp::GreaterEqual,
                TokenKind::Spaceship => BinaryOp::Spaceship,
                TokenKind::Instanceof => {
                    self.advance();
                    let right = self.parse_pipe()?;
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
            let right = self.parse_pipe()?;
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

    /// `.` binds looser than the shifts and `+`/`-` (PHP 8).
    fn parse_concat(&mut self) -> ParseResult<Expr> {
        let mut left = self.parse_shift()?;
        while matches!(self.peek(), TokenKind::Dot) {
            self.advance();
            let right = self.parse_shift()?;
            left = Expr {
                span: left.span.merge(right.span),
                kind: ExprKind::BinaryOp {
                    op: BinaryOp::Concat,
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
                let operand = self.parse_pow()?;
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
                let operand = self.parse_pow()?;
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
                let mut operand = self.parse_pow()?;
                // `instanceof` binds tighter than `!`: !$a instanceof B is !($a instanceof B)
                while matches!(self.peek(), TokenKind::Instanceof) {
                    self.advance();
                    let right = self.parse_unary()?;
                    operand = Expr {
                        span: operand.span.merge(right.span),
                        kind: ExprKind::Instanceof {
                            expr: Box::new(operand),
                            class: Box::new(right),
                        },
                    };
                }
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
                let operand = self.parse_pow()?;
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
                let operand = self.parse_pow()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::Cast(CastType::Int, Box::new(operand)),
                })
            }
            TokenKind::FloatCast => {
                self.advance();
                let operand = self.parse_pow()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::Cast(CastType::Float, Box::new(operand)),
                })
            }
            TokenKind::StringCast => {
                self.advance();
                let operand = self.parse_pow()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::Cast(CastType::String, Box::new(operand)),
                })
            }
            TokenKind::BoolCast => {
                self.advance();
                let operand = self.parse_pow()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::Cast(CastType::Bool, Box::new(operand)),
                })
            }
            TokenKind::ArrayCast => {
                self.advance();
                let operand = self.parse_pow()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::Cast(CastType::Array, Box::new(operand)),
                })
            }
            TokenKind::ObjectCast => {
                self.advance();
                let operand = self.parse_pow()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::Cast(CastType::Object, Box::new(operand)),
                })
            }
            TokenKind::RealCast => Err(ParseError {
                message: "The (real) cast has been removed, use (float) instead".to_string(),
                span,
            }),
            TokenKind::At => {
                self.advance();
                let operand = self.parse_pow()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::Suppress(Box::new(operand)),
                })
            }
            TokenKind::Clone => {
                self.advance();
                // PHP 8.5 `clone` (zend_language_parser.y): `T_CLONE expr` and
                // `T_CLONE clone_argument_list` both compile to a call of the
                // `\clone()` function. `clone(expr)` with a single bare
                // expression is the language construct on a parenthesized
                // operand (so postfix operations continue on it); an argument
                // list needs `()`, `(...)`, a named / unpacked first argument
                // or a `,` after the first expression.
                let clone_name = |span| Expr {
                    span,
                    kind: ExprKind::Identifier(b"\\clone".to_vec()),
                };
                if matches!(self.peek(), TokenKind::OpenParen) {
                    let saved_pos = self.pos;
                    self.advance(); // consume '('
                    let call_form = match self.peek() {
                        TokenKind::CloseParen | TokenKind::Ellipsis => true,
                        TokenKind::Variable(_) => false,
                        _ => matches!(self.peek_at(1), TokenKind::Colon),
                    };
                    let args = if call_form {
                        Some(self.parse_arguments()?)
                    } else {
                        let first = self.parse_expression()?;
                        if self.eat(&TokenKind::Comma) {
                            let mut args = vec![Argument {
                                name: None,
                                value: first,
                                unpack: false,
                            }];
                            args.extend(self.parse_arguments()?);
                            Some(args)
                        } else {
                            None
                        }
                    };
                    if let Some(mut args) = args {
                        let is_fcc = self.first_class_callable_flag;
                        let end_span = self.span();
                        self.expect(&TokenKind::CloseParen)?;
                        let full = span.merge(end_span);
                        if is_fcc {
                            return Ok(Expr {
                                span: full,
                                kind: ExprKind::FirstClassCallable(CallableTarget::Function(
                                    Box::new(clone_name(span)),
                                )),
                            });
                        }
                        // `clone($x,)`: the construct (ZEND_CLONE).
                        if args.len() == 1 && args[0].name.is_none() && !args[0].unpack {
                            let a = args.pop().unwrap();
                            return Ok(Expr {
                                span: full,
                                kind: ExprKind::Clone(Box::new(a.value)),
                            });
                        }
                        return Ok(Expr {
                            span: full,
                            kind: ExprKind::FunctionCall {
                                name: Box::new(clone_name(span)),
                                args,
                            },
                        });
                    }
                    // A single parenthesized operand: rewind before the '('
                    // and let parse_unary handle the full postfix chain.
                    self.pos = saved_pos;
                }
                let start = self.pos;
                let mut operand = self.parse_unary()?;
                // `clone $a = expr` is `clone ($a = expr)` (the grammar shifts
                // the assignment onto the variable).
                if Self::is_lvalue(&operand)
                    && matches!(
                        self.peek(),
                        TokenKind::Assign
                            | TokenKind::PlusAssign
                            | TokenKind::MinusAssign
                            | TokenKind::StarAssign
                            | TokenKind::SlashAssign
                            | TokenKind::PercentAssign
                            | TokenKind::PowAssign
                            | TokenKind::DotAssign
                            | TokenKind::AmpersandAssign
                            | TokenKind::PipeAssign
                            | TokenKind::CaretAssign
                            | TokenKind::ShiftLeftAssign
                            | TokenKind::ShiftRightAssign
                            | TokenKind::NullCoalesceAssign
                    )
                {
                    self.pos = start;
                    operand = self.parse_assignment()?;
                }
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

    /// `X::$prop::get(...)` / `X::$prop::set(...)` after the property name:
    /// lowered to `X::__property_{get,set}_<prop>(...)`. Returns `Err(class)`
    /// (untouched) when the tokens don't form a hook call.
    fn parse_prop_hook_call(&mut self, expr: Expr, name: &[u8]) -> ParseResult<Result<Expr, Expr>> {
        if !matches!(self.peek(), TokenKind::DoubleColon) {
            return Ok(Err(expr));
        }
        let lookahead = self.tokens.get(self.pos + 1).map(|t| t.kind.clone());
        let Some(TokenKind::Identifier(ref hook_name)) = lookahead else {
            return Ok(Err(expr));
        };
        if !(hook_name.eq_ignore_ascii_case(b"get") || hook_name.eq_ignore_ascii_case(b"set"))
            || !matches!(
                self.tokens.get(self.pos + 2).map(|t| &t.kind),
                Some(TokenKind::OpenParen)
            )
        {
            return Ok(Err(expr));
        }
        self.advance(); // ::
        let is_set = hook_name.eq_ignore_ascii_case(b"set");
        self.advance(); // get/set
        self.advance(); // (
        let args = self.parse_arguments()?;
        let is_fcc = self.first_class_callable_flag;
        let end_span = self.span();
        self.expect(&TokenKind::CloseParen)?;
        let mut synthetic_method = if is_set {
            b"__property_set_".to_vec()
        } else {
            b"__property_get_".to_vec()
        };
        synthetic_method.extend_from_slice(name);
        Ok(Ok(if is_fcc {
            Expr {
                span: expr.span.merge(end_span),
                kind: ExprKind::FirstClassCallable(CallableTarget::StaticMethod {
                    class: Box::new(expr),
                    method: synthetic_method,
                }),
            }
        } else {
            Expr {
                span: expr.span.merge(end_span),
                kind: ExprKind::StaticMethodCall {
                    class: Box::new(expr),
                    method: synthetic_method,
                    args,
                },
            }
        }))
    }

    /// simple_variable: `$v`, `${expr}`, `$$v`, or `$` before another
    /// simple variable (`$$$v`, `$${expr}`).
    fn parse_simple_variable(&mut self) -> ParseResult<Expr> {
        let span = self.span();
        match self.peek().clone() {
            TokenKind::Variable(name) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Variable(name),
                    span,
                })
            }
            TokenKind::VariableVariable(name) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::DynamicVariable(Box::new(Expr {
                        kind: ExprKind::Variable(name),
                        span,
                    })),
                    span,
                })
            }
            TokenKind::DollarOpenBrace => {
                self.advance();
                let inner = self.parse_expression()?;
                self.expect(&TokenKind::CloseBrace)?;
                // `${'name'}` names a compiled variable, like `$name`
                // (zend_try_compile_cv).
                if let ExprKind::String(name) = &inner.kind {
                    let valid = name
                        .first()
                        .is_some_and(|&c| c.is_ascii_alphabetic() || c == b'_' || c >= 0x80)
                        && name
                            .iter()
                            .all(|&c| c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80);
                    if valid {
                        return Ok(Expr {
                            kind: ExprKind::Variable(name.clone()),
                            span,
                        });
                    }
                }
                Ok(Expr {
                    kind: ExprKind::DynamicVariable(Box::new(inner)),
                    span,
                })
            }
            TokenKind::Dollar => {
                self.advance();
                let inner = self.parse_simple_variable()?;
                Ok(Expr {
                    kind: ExprKind::DynamicVariable(Box::new(inner)),
                    span,
                })
            }
            _ => Err(ParseError {
                message: "expected variable".into(),
                span,
            }),
        }
    }

    /// The member after `->` / `?->` (property_name / member_name):
    /// an identifier, `{expr}`, or a simple variable (`$v`, `${expr}`,
    /// `$$v`), then a method call when followed by `(`.
    fn parse_object_member(&mut self, object: Expr, nullsafe: bool) -> ParseResult<Expr> {
        let prop_span = self.span();
        let member = match self.peek().clone() {
            TokenKind::OpenBrace => {
                self.advance();
                let e = self.parse_expression()?;
                self.expect(&TokenKind::CloseBrace)?;
                e
            }
            TokenKind::Variable(_)
            | TokenKind::DollarOpenBrace
            | TokenKind::VariableVariable(_)
            | TokenKind::Dollar => self.parse_simple_variable()?,
            TokenKind::Identifier(name) => {
                self.advance();
                Expr {
                    kind: ExprKind::Identifier(name),
                    span: prop_span,
                }
            }
            _ if self.is_semi_reserved_keyword() => {
                let kw = self.keyword_to_identifier();
                self.advance();
                Expr {
                    kind: ExprKind::Identifier(kw),
                    span: prop_span,
                }
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
            let span = object.span.merge(end_span);
            return Ok(if is_fcc {
                Expr {
                    span,
                    kind: ExprKind::FirstClassCallable(CallableTarget::Method {
                        object: Box::new(object),
                        method: Box::new(member),
                        nullsafe,
                    }),
                }
            } else {
                Expr {
                    span,
                    kind: ExprKind::MethodCall {
                        object: Box::new(object),
                        method: Box::new(member),
                        args,
                        nullsafe,
                    },
                }
            });
        }
        Ok(Expr {
            span: object.span.merge(member.span),
            kind: ExprKind::PropertyAccess {
                object: Box::new(object),
                property: Box::new(member),
                nullsafe,
            },
        })
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
                TokenKind::Arrow | TokenKind::NullsafeArrow => {
                    let nullsafe = matches!(self.peek(), TokenKind::NullsafeArrow);
                    self.advance();
                    expr = self.parse_object_member(expr, nullsafe)?;
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
                                    kind: ExprKind::FirstClassCallable(
                                        CallableTarget::StaticMethod {
                                            class: Box::new(expr),
                                            method: name,
                                        },
                                    ),
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
                        // `X::class` (but `X::class()` is a method named "class")
                        TokenKind::Class
                            if !matches!(
                                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                                Some(TokenKind::OpenParen)
                            ) =>
                        {
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
                            // Property hook parent call: parent::$prop::get() / parent::$prop::set($v)
                            expr = match self.parse_prop_hook_call(expr, &name)? {
                                Ok(call) => {
                                    expr = call;
                                    continue;
                                }
                                Err(class) => class,
                            };
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
                        TokenKind::DollarOpenBrace => {
                            // Test::${expr} - static property named by expression
                            // `parent::${0}::get()`: a literal name is a hook call
                            // like `parent::$name::get()`.
                            let literal = match self.tokens.get(self.pos + 1).map(|t| &t.kind) {
                                Some(TokenKind::LongNumber(n)) => Some(n.to_string().into_bytes()),
                                Some(TokenKind::ConstantString(s)) => Some(s.clone()),
                                _ => None,
                            };
                            if let Some(lit) = literal.filter(|_| {
                                matches!(
                                    self.tokens.get(self.pos + 2).map(|t| &t.kind),
                                    Some(TokenKind::CloseBrace)
                                ) && matches!(
                                    self.tokens.get(self.pos + 3).map(|t| &t.kind),
                                    Some(TokenKind::DoubleColon)
                                )
                            }) {
                                let saved = self.pos;
                                self.pos += 3;
                                match self.parse_prop_hook_call(expr, &lit)? {
                                    Ok(call) => {
                                        expr = call;
                                        continue;
                                    }
                                    Err(class) => {
                                        expr = class;
                                        self.pos = saved;
                                    }
                                }
                            }
                            self.advance();
                            let name_expr = self.parse_expression()?;
                            self.expect(&TokenKind::CloseBrace)?;
                            expr = Expr {
                                span: expr.span.merge(member_span),
                                kind: ExprKind::DynamicStaticPropertyAccess {
                                    class: Box::new(expr),
                                    property: Box::new(name_expr),
                                },
                            };
                        }
                        TokenKind::Dollar => {
                            // Test::$$$bar / Test::$${expr}: the name is the
                            // value of the simple variable after the `$`.
                            self.advance();
                            let name_expr = self.parse_simple_variable()?;
                            expr = Expr {
                                span: expr.span.merge(member_span),
                                kind: ExprKind::DynamicStaticPropertyAccess {
                                    class: Box::new(expr),
                                    property: Box::new(name_expr),
                                },
                            };
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
                                        kind: ExprKind::FirstClassCallable(
                                            CallableTarget::StaticMethod {
                                                class: Box::new(expr),
                                                method: name,
                                            },
                                        ),
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
                            if matches!(self.peek(), TokenKind::OpenParen) {
                                // Foo::{'method'}(...) is a static call with a dynamic name
                                self.advance();
                                let args = self.parse_arguments()?;
                                let end_span = self.span();
                                self.expect(&TokenKind::CloseParen)?;
                                expr = Expr {
                                    span: expr.span.merge(end_span),
                                    kind: ExprKind::DynamicStaticMethodCall {
                                        class: Box::new(expr),
                                        method: Box::new(inner),
                                        args,
                                    },
                                };
                                continue;
                            }
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
                            kind: ExprKind::FirstClassCallable(CallableTarget::Function(Box::new(
                                expr,
                            ))),
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

        // `variable '=' ampersand variable` is itself an expression: the
        // right side is a variable, and what follows applies to the whole
        // assignment (`$r = &$o->p + 1` is `($r = &$o->p) + 1`).
        if Self::is_lvalue(&expr)
            && matches!(self.peek(), TokenKind::Assign)
            && matches!(self.peek_at(1), TokenKind::Ampersand)
        {
            self.advance();
            self.advance();
            let right = self.parse_postfix()?;
            // A class constant cannot be referenced (`$a = &C::X`).
            if matches!(right.kind, ExprKind::ClassConstAccess { .. }) {
                return Err(ParseError {
                    message: "syntax error, unexpected token \";\", expecting \"->\" or \"?->\" or \"{\" or \"[\"".into(),
                    span: self.span(),
                });
            }
            return Ok(Expr {
                span: expr.span.merge(right.span),
                kind: ExprKind::AssignRef {
                    target: Box::new(expr),
                    value: Box::new(right),
                },
            });
        }

        Ok(expr)
    }

    fn parse_primary(&mut self) -> ParseResult<Expr> {
        let span = self.span();
        match self.peek().clone() {
            TokenKind::AttributeOpen => {
                let attr_doc = self.take_doc_comment();
                let attributes = self.parse_attributes()?;
                let span = self.span();
                // `#[A] static function () {}` / `#[A] static fn () => ...`
                let is_static = matches!(self.peek(), TokenKind::Static)
                    && matches!(self.peek_at(1), TokenKind::Function | TokenKind::Fn);
                if is_static {
                    self.advance();
                }
                match self.peek() {
                    TokenKind::Function => {
                        self.advance();
                        let by_ref_return = self.eat(&TokenKind::Ampersand);
                        self.expect(&TokenKind::OpenParen)?;
                        let params = self.parse_params()?;
                        self.expect(&TokenKind::CloseParen)?;
                        let use_vars = if matches!(self.peek(), TokenKind::Use) {
                            self.advance();
                            self.expect(&TokenKind::OpenParen)?;
                            let mut v = Vec::new();
                            loop {
                                let br = self.eat(&TokenKind::Ampersand);
                                match self.peek().clone() {
                                    TokenKind::Variable(n) => {
                                        self.advance();
                                        v.push(ClosureUse {
                                            variable: n,
                                            by_ref: br,
                                        });
                                    }
                                    _ => break,
                                };
                                if !self.eat(&TokenKind::Comma) {
                                    break;
                                }
                            }
                            self.expect(&TokenKind::CloseParen)?;
                            v
                        } else {
                            Vec::new()
                        };
                        let return_type = if self.eat(&TokenKind::Colon) {
                            Some(self.parse_type_hint()?)
                        } else {
                            None
                        };
                        let (body, end_line) = self.parse_block_with_end()?;
                        Ok(Expr {
                            span,
                            kind: ExprKind::Closure {
                                is_static,
                                params,
                                use_vars,
                                return_type,
                                body,
                                attributes,
                                end_line,
                                doc_comment: attr_doc,
                                returns_by_ref: by_ref_return,
                            },
                        })
                    }
                    TokenKind::Fn => {
                        self.advance();
                        let by_ref_return = self.eat(&TokenKind::Ampersand);
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
                        let end_line = body.span.line;
                        Ok(Expr {
                            span,
                            kind: ExprKind::ArrowFunction {
                                is_static,
                                params,
                                return_type,
                                body: Box::new(body),
                                attributes,
                                end_line,
                                doc_comment: attr_doc,
                                parenthesized: false,
                                returns_by_ref: by_ref_return,
                            },
                        })
                    }
                    _ => Err(ParseError {
                        message: "expected function or fn after attribute".into(),
                        span: self.span(),
                    }),
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
                            // Parse chained property/method/array access
                            loop {
                                if matches!(self.peek(), TokenKind::Arrow) {
                                    self.advance(); // consume ->
                                    if let TokenKind::Identifier(prop_name) = self.peek().clone() {
                                        self.advance();
                                        // Method call: identifier followed by `(`
                                        if matches!(self.peek(), TokenKind::OpenParen) {
                                            self.advance(); // consume (
                                            let mut args: Vec<crate::ast::Argument> = Vec::new();
                                            while !matches!(
                                                self.peek(),
                                                TokenKind::CloseParen | TokenKind::Eof
                                            ) {
                                                let arg_expr = self.parse_expression()?;
                                                args.push(crate::ast::Argument {
                                                    name: None,
                                                    value: arg_expr,
                                                    unpack: false,
                                                });
                                                if matches!(self.peek(), TokenKind::Comma) {
                                                    self.advance();
                                                } else {
                                                    break;
                                                }
                                            }
                                            if matches!(self.peek(), TokenKind::CloseParen) {
                                                self.advance();
                                            }
                                            expr = Expr {
                                                kind: ExprKind::MethodCall {
                                                    object: Box::new(expr),
                                                    method: Box::new(Expr {
                                                        kind: ExprKind::Identifier(prop_name),
                                                        span: var_span,
                                                    }),
                                                    args,
                                                    nullsafe: false,
                                                },
                                                span: var_span,
                                            };
                                        } else {
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
                                        }
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
                        TokenKind::InterpExprStart => {
                            // "{$expr}": a full expression up to the `}`.
                            self.advance();
                            let expr = self.parse_expression()?;
                            self.expect(&TokenKind::InterpExprEnd)?;
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
            TokenKind::DollarOpenBrace | TokenKind::Dollar => self.parse_simple_variable(),
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
                            kind: ExprKind::FirstClassCallable(CallableTarget::Function(Box::new(
                                name_expr,
                            ))),
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
                let mut expr = self.parse_expression()?;
                self.expect(&TokenKind::CloseParen)?;
                // `(fn() => ...)` keeps its parentheses in AST exports
                // (not as a callee: `(fn() => ...)()`).
                if let ExprKind::ArrowFunction { parenthesized, .. } = &mut expr.kind {
                    if !matches!(self.peek(), TokenKind::OpenParen) {
                        *parenthesized = true;
                    }
                }
                // `(NAME)(...)` calls the value of constant NAME, not a
                // function called NAME: mark the name as a constant fetch.
                if let ExprKind::Identifier(name) = &expr.kind {
                    if matches!(self.peek(), TokenKind::OpenParen) {
                        return Ok(Expr {
                            kind: ExprKind::ConstantAccess(vec![name.clone()]),
                            span: expr.span,
                        });
                    }
                }
                Ok(expr)
            }
            TokenKind::OpenBracket => {
                self.advance();
                let mut elements = Vec::new();
                while !matches!(self.peek(), TokenKind::CloseBracket | TokenKind::Eof) {
                    // Handle empty elements (for list destructuring like [$a, , $c])
                    if matches!(self.peek(), TokenKind::Comma) {
                        // Zero-width span marks an empty placeholder element
                        // (distinguishable from a literal `null`).
                        let prev = self.tokens[self.pos.saturating_sub(1)].span;
                        elements.push(ArrayElement {
                            key: None,
                            value: Expr {
                                span: Span::new(prev.end, prev.end, prev.line),
                                kind: ExprKind::Null,
                            },
                            unpack: false,
                            by_ref: false,
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
                            by_ref: false,
                        });
                    } else {
                        // Handle &$var (reference in array)
                        let is_ref = self.eat(&TokenKind::Ampersand);
                        let first = self.parse_expression()?;
                        if self.eat(&TokenKind::DoubleArrow) {
                            let is_val_ref = self.eat(&TokenKind::Ampersand);
                            let value = self.parse_expression()?;
                            elements.push(ArrayElement {
                                key: Some(first),
                                value,
                                unpack: false,
                                by_ref: is_val_ref,
                            });
                        } else {
                            elements.push(ArrayElement {
                                key: None,
                                value: first,
                                unpack: false,
                                by_ref: is_ref,
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
                    let is_ref = self.eat(&TokenKind::Ampersand);
                    let first = self.parse_expression()?;
                    if self.eat(&TokenKind::DoubleArrow) {
                        let is_val_ref = self.eat(&TokenKind::Ampersand);
                        let value = self.parse_expression()?;
                        elements.push(ArrayElement {
                            key: Some(first),
                            value,
                            unpack: false,
                            by_ref: is_val_ref,
                        });
                    } else {
                        elements.push(ArrayElement {
                            key: None,
                            value: first,
                            unpack: false,
                            by_ref: is_ref,
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
                let anon_attrs = if matches!(self.peek(), TokenKind::AttributeOpen) {
                    self.parse_attributes()?
                } else {
                    Vec::new()
                };
                // Check for class modifiers before anonymous class: new readonly class, new abstract class, etc.
                let mut anon_modifiers = ClassModifiers::default();
                let mut has_anon_modifiers = false;
                loop {
                    match self.peek() {
                        TokenKind::Readonly
                            if matches!(
                                self.peek_at(1),
                                TokenKind::Class
                                    | TokenKind::Readonly
                                    | TokenKind::Abstract
                                    | TokenKind::Final
                            ) =>
                        {
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
                        TokenKind::Abstract
                            if matches!(
                                self.peek_at(1),
                                TokenKind::Class
                                    | TokenKind::Readonly
                                    | TokenKind::Abstract
                                    | TokenKind::Final
                            ) =>
                        {
                            anon_modifiers.is_abstract = true;
                            has_anon_modifiers = true;
                            self.advance();
                        }
                        TokenKind::Final
                            if matches!(
                                self.peek_at(1),
                                TokenKind::Class
                                    | TokenKind::Readonly
                                    | TokenKind::Abstract
                                    | TokenKind::Final
                            ) =>
                        {
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
                            message: "Cannot use the abstract modifier on an anonymous class"
                                .into(),
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
                    // `new namespace\Foo`: a namespace-relative name.
                    TokenKind::Namespace
                        if matches!(self.peek_at(1), TokenKind::Backslash)
                            && matches!(self.peek_at(2), TokenKind::Identifier(_)) =>
                    {
                        self.advance();
                        let mut full_name = b"namespace".to_vec();
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
                            if self.first_class_callable_flag {
                                return Err(ParseError {
                                    message: "Cannot create Closure for new expression".into(),
                                    span: self.span(),
                                });
                            }
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
                        // Generate unique anonymous class name
                        // CG(rtd_key_counter) starts at 0
                        let anon_name = format!("__anonymous_class_{}", self.anon_counter);
                        self.anon_counter += 1;
                        // Parse class body with class name on stack for error messages
                        self.class_name_stack.push(anon_name.as_bytes().to_vec());
                        let body_result = self.parse_class_body();
                        self.class_name_stack.pop();
                        let (body, end_line) = body_result?;
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
                                attributes: anon_attrs,
                                end_line,
                                doc_comment: None,
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
                        while let TokenKind::Identifier(part) = self.peek().clone() {
                            self.advance();
                            if full_name.len() > 1 {
                                full_name.push(b'\\');
                            }
                            full_name.extend_from_slice(&part);
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
                    if self.first_class_callable_flag {
                        return Err(ParseError {
                            message: "Cannot create Closure for new expression".into(),
                            span,
                        });
                    }
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
                    if matches!(self.peek(), TokenKind::CloseParen) {
                        break;
                    }
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
                // PHP 8.4: exit/die is a function; `exit` without parentheses
                // and `exit(<one positional arg>)` keep the fast path, other
                // argument lists (named, unpacked, `...`) call \exit().
                let value = if matches!(self.peek(), TokenKind::OpenParen) {
                    self.advance();
                    let args = self.parse_arguments()?;
                    let is_fcc = self.first_class_callable_flag;
                    let end_span = self.span();
                    self.expect(&TokenKind::CloseParen)?;
                    let name_expr = Expr {
                        kind: ExprKind::Identifier(b"exit".to_vec()),
                        span,
                    };
                    if is_fcc {
                        return Ok(Expr {
                            span: span.merge(end_span),
                            kind: ExprKind::FirstClassCallable(CallableTarget::Function(Box::new(
                                name_expr,
                            ))),
                        });
                    }
                    match args.len() {
                        0 => None,
                        1 if args[0].name.is_none() && !args[0].unpack => {
                            Some(Box::new(args.into_iter().next().unwrap().value))
                        }
                        _ => {
                            return Ok(Expr {
                                span: span.merge(end_span),
                                kind: ExprKind::FunctionCall {
                                    name: Box::new(name_expr),
                                    args,
                                },
                            });
                        }
                    }
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
                // Anonymous function. Prefer a stashed closure doc-comment
                // from an enclosing `$var = function(){}` assignment, falling
                // back to a doc-comment directly attached to this token.
                let closure_doc = self
                    .pending_closure_doc
                    .take()
                    .or_else(|| self.take_doc_comment());
                self.advance();
                // Optional & for reference return
                let by_ref_return = self.eat(&TokenKind::Ampersand);
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

                let (body, end_line) = self.parse_block_with_end()?;

                Ok(Expr {
                    span,
                    kind: ExprKind::Closure {
                        is_static: false,
                        params,
                        use_vars,
                        return_type,
                        body,
                        attributes: Vec::new(),
                        end_line,
                        doc_comment: closure_doc,
                        returns_by_ref: by_ref_return,
                    },
                })
            }
            TokenKind::Fn
                if !matches!(
                    self.tokens.get(self.pos + 1).map(|t| &t.kind),
                    Some(TokenKind::Backslash)
                ) =>
            {
                let arrow_doc = self
                    .pending_closure_doc
                    .take()
                    .or_else(|| self.take_doc_comment());
                self.advance();
                // Optional & for reference return
                let by_ref_return = self.eat(&TokenKind::Ampersand);
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
                let end_line = body.span.line;
                Ok(Expr {
                    span,
                    kind: ExprKind::ArrowFunction {
                        is_static: false,
                        params,
                        return_type,
                        body: Box::new(body),
                        attributes: Vec::new(),
                        end_line,
                        doc_comment: arrow_doc,
                        parenthesized: false,
                        returns_by_ref: by_ref_return,
                    },
                })
            }
            TokenKind::Static
                if matches!(
                    self.tokens.get(self.pos + 1).map(|t| &t.kind),
                    Some(TokenKind::Function | TokenKind::Fn)
                ) =>
            {
                let static_doc = self
                    .pending_closure_doc
                    .take()
                    .or_else(|| self.take_doc_comment());
                self.advance(); // static
                if matches!(self.peek(), TokenKind::Fn) {
                    self.advance();
                    let by_ref_return = self.eat(&TokenKind::Ampersand);
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
                    let end_line = body.span.line;
                    Ok(Expr {
                        span,
                        kind: ExprKind::ArrowFunction {
                            is_static: true,
                            params,
                            return_type,
                            body: Box::new(body),
                            attributes: Vec::new(),
                            end_line,
                            doc_comment: static_doc,
                            parenthesized: false,
                            returns_by_ref: by_ref_return,
                        },
                    })
                } else {
                    self.advance(); // function
                    let by_ref_return = self.eat(&TokenKind::Ampersand);
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
                    let (body, end_line) = self.parse_block_with_end()?;
                    Ok(Expr {
                        span,
                        kind: ExprKind::Closure {
                            is_static: true,
                            params,
                            use_vars,
                            return_type,
                            body,
                            attributes: Vec::new(),
                            end_line,
                            doc_comment: static_doc,
                            returns_by_ref: by_ref_return,
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
                // Tokens after which yield is bare (no value follows). Includes
                // statement/expr terminators and ALL binary operators that bind
                // tighter than yield (yield is one of the loosest expressions in
                // PHP). A binary operator token here means we're computing
                // `<binop>(yield, rhs)` and yield is the LHS.
                let bare = matches!(
                    self.peek(),
                    TokenKind::Semicolon
                        | TokenKind::CloseParen
                        | TokenKind::CloseBracket
                        | TokenKind::CloseBrace
                        | TokenKind::Comma
                        | TokenKind::Colon
                        | TokenKind::QuestionMark
                        | TokenKind::DoubleArrow
                        | TokenKind::Star
                        | TokenKind::Slash
                        | TokenKind::Percent
                        | TokenKind::Pow
                        | TokenKind::Dot
                        | TokenKind::Equal
                        | TokenKind::Identical
                        | TokenKind::NotEqual
                        | TokenKind::NotIdentical
                        | TokenKind::Less
                        | TokenKind::Greater
                        | TokenKind::LessEqual
                        | TokenKind::GreaterEqual
                        | TokenKind::Spaceship
                        | TokenKind::BooleanAnd
                        | TokenKind::BooleanOr
                        | TokenKind::Ampersand
                        | TokenKind::Pipe
                        | TokenKind::Caret
                        | TokenKind::ShiftLeft
                        | TokenKind::ShiftRight
                        | TokenKind::NullCoalesce
                        | TokenKind::And
                        | TokenKind::Or
                        | TokenKind::Xor
                );
                if bare {
                    Ok(Expr {
                        span,
                        kind: ExprKind::Yield(None, None),
                    })
                } else {
                    // yield binds tighter than `and`/`xor`/`or`.
                    let first = self.parse_assignment()?;
                    // Check for yield $key => $value
                    if matches!(self.peek(), TokenKind::DoubleArrow) {
                        self.advance(); // consume =>
                        let value = self.parse_assignment()?;
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
                            kind: ExprKind::FirstClassCallable(CallableTarget::Function(Box::new(
                                name_expr,
                            ))),
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
                                kind: ExprKind::FirstClassCallable(CallableTarget::Function(
                                    Box::new(name_expr),
                                )),
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
                message: format!(
                    "syntax error, unexpected {}",
                    self.peek().to_php_unexpected()
                ),
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

        // A nested `inner(...)` must not mark this (outer) call as an FCC.
        self.first_class_callable_flag = false;
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
                    by_ref: false,
                });
                if self.eat(&TokenKind::Comma) {
                    if matches!(self.peek(), TokenKind::CloseParen) {
                        break;
                    }
                    continue;
                }
                break;
            }

            let first_ref = self.eat(&TokenKind::Ampersand);
            let first = self.parse_expression()?;
            if !first_ref && self.eat(&TokenKind::DoubleArrow) {
                // Keyed list: "key" => $var or "key" => list(...)
                let value_ref = self.eat(&TokenKind::Ampersand);
                let value = self.parse_expression()?;
                elements.push(ArrayElement {
                    key: Some(first),
                    value,
                    unpack: false,
                    by_ref: value_ref,
                });
            } else {
                elements.push(ArrayElement {
                    key: None,
                    value: first,
                    unpack: false,
                    by_ref: first_ref,
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

    fn is_kind_semi_reserved(kind: &TokenKind) -> bool {
        matches!(
            kind,
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
    /// Attach the source the tokens were lexed from (see `source`).
    pub fn with_source(mut self, source: &[u8]) -> Self {
        self.source = source.to_vec();
        self
    }

    fn keyword_to_identifier(&self) -> Vec<u8> {
        let kw = self.keyword_to_identifier_lower();
        // Keep the spelling as written when the source is known.
        if !kw.is_empty() {
            let span = self.span();
            if let Some(text) = self.source.get(span.start as usize..span.end as usize) {
                if text.eq_ignore_ascii_case(&kw) {
                    return text.to_vec();
                }
            }
        }
        kw
    }

    fn keyword_to_identifier_lower(&self) -> Vec<u8> {
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
            TokenKind::Exit => {
                if span_len == 3 {
                    b"die".to_vec()
                } else {
                    b"exit".to_vec()
                }
            }
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

/// Check whether a set-hook parameter type is compatible with a property type
/// for the purposes of PHP's LSP-contravariance rule for property hooks.
///
/// Returns true if the parameter type is equal to or a supertype of the
/// property type (which is what PHP requires). We use a conservative
/// structural-equality + widening heuristic that is correct for the common
/// cases at parse time, when we don't yet have access to the class hierarchy.
fn set_param_type_compatible(prop: &TypeHint, param: &TypeHint) -> bool {
    fn type_display(t: &TypeHint) -> String {
        match t {
            TypeHint::Simple(n) => String::from_utf8_lossy(n).to_ascii_lowercase(),
            TypeHint::Nullable(inner) => format!("?{}", type_display(inner)),
            TypeHint::Union(types) => {
                let mut parts: Vec<String> = types.iter().map(type_display).collect();
                parts.sort();
                parts.join("|")
            }
            TypeHint::Intersection(types) => {
                let mut parts: Vec<String> = types.iter().map(type_display).collect();
                parts.sort();
                parts.join("&")
            }
        }
    }

    // Identical types are always compatible
    if type_display(prop) == type_display(param) {
        return true;
    }

    // `mixed` parameter always accepts any property type
    if matches!(param, TypeHint::Simple(n) if n.eq_ignore_ascii_case(b"mixed")) {
        return true;
    }

    // Property is nullable T, param is also nullable (or T?) - same base type
    if let (TypeHint::Nullable(pi), TypeHint::Nullable(pai)) = (prop, param) {
        return set_param_type_compatible(pi, pai);
    }

    // Helper: collapse a type to the list of its alternatives.
    fn alternatives(t: &TypeHint) -> Vec<&TypeHint> {
        match t {
            TypeHint::Union(parts) => parts.iter().collect(),
            _ => vec![t],
        }
    }

    // For the param type to be compatible, every alternative of the property type
    // must also appear in the param type (so the set hook can accept every
    // possible stored value). We match alternatives structurally.
    let prop_alts = alternatives(prop);
    let param_alts = alternatives(param);
    for pa in &prop_alts {
        let pa_disp = type_display(pa);
        if !param_alts.iter().any(|pp| type_display(pp) == pa_disp) {
            // For simple class-name vs simple class-name we can't tell
            // without the class hierarchy: allow it to avoid false positives.
            if prop_alts.len() == 1 && param_alts.len() == 1 {
                if let (TypeHint::Simple(_), TypeHint::Simple(_)) = (prop, param) {
                    return true;
                }
            }
            return false;
        }
    }
    true
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
