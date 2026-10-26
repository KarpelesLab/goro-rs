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

const LIST_SYNTAX_LIST: u8 = 1;
const LIST_SYNTAX_SHORT: u8 = 2;
const LIST_SYNTAX_LONG: u8 = 3;

pub struct Parser {
    /// Syntax of array literals by (start, end) span: LIST_SYNTAX_*.
    array_syntax: std::collections::HashMap<(u32, u32), u8, SpanHash>,
    /// Nesting of `[...]` / `list(...)` literals being parsed.
    array_literal_depth: u32,
    /// Parsing a foreach target (destructuring without `=`).
    in_foreach_target: bool,
    tokens: Vec<Token>,
    /// Doc comments by the index of the token they are attached to (in
    /// increasing order). A comment immediately precedes its token in source
    /// (with only whitespace between); `take_doc_comment()` consumes them.
    token_doc_comments: Vec<(u32, Option<Vec<u8>>)>,
    /// take_doc_comment() cleared every doc comment up to this token.
    doc_floor: usize,
    pos: usize,
    depth: u32,
    /// Anonymous class declarations that need to be emitted before the current statement
    anon_class_stmts: Vec<Statement>,
    /// Counter for generating unique anonymous class names
    anon_counter: u32,
    /// Set to true by parse_arguments() when it detects first-class callable syntax (...)
    first_class_callable_flag: bool,
    /// Set by parse_primary() for `new Foo` without an argument list
    /// (new_non_dereferenceable): no member access may follow it.
    new_non_dereferenceable: bool,
    /// Pending extra constants from comma-separated const declarations
    pending_extra_constants: Vec<ClassMember>,
    /// The source text (when given with `with_source`): keywords used as
    /// identifiers keep their spelling (`A::Case`, `$o->Default`).
    source: Vec<u8>,
    /// The source is known to be valid (see `trusted`): the grammar
    /// recognizer does not check it.
    trusted: bool,
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
    /// Token index of the `)` that closed the most recent grouping
    /// `( expr )` (not a call's argument list): an expression ending there
    /// is not a `variable` in PHP's grammar, so it cannot be assigned to.
    last_group_close: usize,
    /// Nesting of parse_expression() / parse_unary(): every
    /// STACK_CHECK_EVERY-th level makes sure of native stack room.
    stack_nesting: u32,
    /// Nesting of speculate(): what is parsed may be parsed again, so
    /// advance_take_bytes() copies the payloads instead of taking them.
    speculative: u32,
    /// A `yield` / `yield from` was parsed (Program::has_yield).
    saw_yield: bool,
}

/// See Parser::stack_nesting: the levels in between use far less than the
/// room a check leaves.
const STACK_CHECK_EVERY: u32 = 4;

const MAX_PARSE_DEPTH: u32 = 512;

/// Hasher of the (start, end) span keys of Parser::array_syntax.
#[derive(Default)]
struct SpanHasher(u64);

impl std::hash::Hasher for SpanHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }
    fn write_u32(&mut self, n: u32) {
        self.write_u64(n as u64);
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

type SpanHash = std::hash::BuildHasherDefault<SpanHasher>;

/// Runs a closure with enough native stack: the engine installs one that
/// continues on a fresh stack segment when the current one runs low
/// (deeply nested expressions recurse deeply). Returns false when no more
/// stack can be had (the closure did not run).
pub type StackGrower = fn(&mut dyn FnMut()) -> bool;

static STACK_GROWER: std::sync::OnceLock<StackGrower> = std::sync::OnceLock::new();

/// Install the engine's stack grower (see StackGrower).
pub fn set_stack_grower(f: StackGrower) {
    let _ = STACK_GROWER.set(f);
}

/// Run `f` with room on the native stack (None: the stack is exhausted).
fn with_stack<R>(f: impl FnOnce() -> R) -> Option<R> {
    match STACK_GROWER.get() {
        Some(grow) => {
            let mut f = Some(f);
            let mut out = None;
            if grow(&mut || out = f.take().map(|f| f())) {
                out
            } else {
                None
            }
        }
        None => Some(f()),
    }
}

impl Parser {
    /// The index of a top-level `__halt_compiler();` statement's name
    /// token and the byte offset right after it.
    fn find_halt_compiler(tokens: &[Token]) -> Option<(usize, u32)> {
        // (most scripts have none)
        if !tokens.iter().any(
            |t| matches!(&t.kind, TokenKind::Identifier(n) if n.eq_ignore_ascii_case(b"__halt_compiler")),
        ) {
            return None;
        }
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
        halt
    }

    /// The `__COMPILER_HALT_OFFSET__` of a script: where its top-level
    /// `__halt_compiler();` statement ends, None without one.
    pub fn halt_compiler_offset(source: &[u8]) -> Option<u32> {
        let mut lexer = crate::Lexer::new(source);
        let tokens = lexer.tokenize();
        let _ = crate::take_lexer_deprecations();
        let _ = crate::take_lexer_warnings();
        Self::find_halt_compiler(&tokens).map(|(_, o)| o)
    }

    /// `__halt_compiler();` at the top level: drop everything after it and
    /// resolve direct references to `__COMPILER_HALT_OFFSET__` (unqualified
    /// or `\`-qualified) to the byte offset right after the statement.
    fn apply_halt_compiler(
        tokens: &mut Vec<Token>,
        docs: &mut Vec<(u32, Option<Vec<u8>>)>,
        halt_map: Option<std::rc::Rc<dyn Fn(u32) -> u32>>,
    ) {
        let halt = Self::find_halt_compiler(tokens);
        let Some((hi, offset)) = halt else { return };
        // (an offset into the original script under zend.multibyte filters)
        let offset = halt_map.map_or(offset, |f| f(offset));
        // Keep `__halt_compiler ( ) ;` and the Eof token; drop the rest.
        if let Some(eof) = tokens.last().cloned() {
            if tokens.len() > hi + 4 {
                tokens.truncate(hi + 4);
                docs.retain(|&(i, _)| (i as usize) < hi + 4);
                if eof.kind == TokenKind::Eof {
                    tokens.push(eof);
                }
            }
        }
        for j in 0..hi.min(tokens.len()) {
            // `\X` is the global constant; `ns\X` / `namespace\X` are not.
            let is_const = match &tokens[j].kind {
                TokenKind::Identifier(n) => n.as_slice() == b"__COMPILER_HALT_OFFSET__",
                TokenKind::NameFullyQualified(n) => n.as_slice() == b"\\__COMPILER_HALT_OFFSET__",
                _ => false,
            };
            if is_const {
                let prev = if j > 0 {
                    Some(&tokens[j - 1].kind)
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
                    tokens[j].kind = TokenKind::LongNumber(offset as i64);
                }
            }
        }
    }

    pub fn new(tokens: Vec<Token>) -> Self {
        // Strip DocComment tokens out of the stream and attach each to the
        // *next* non-doc token: (i, doc) is the (last) doc comment
        // immediately preceding tokens[i].
        let mut filtered = tokens;
        let mut doc_comments: Vec<(u32, Option<Vec<u8>>)> = Vec::new();
        let mut pending_doc: Option<Vec<u8>> = None;
        let mut index = 0u32;
        filtered.retain_mut(|tok| {
            if let TokenKind::DocComment(bytes) = &mut tok.kind {
                // Keep the most recent doc-comment if multiple appear in a row
                pending_doc = Some(std::mem::take(bytes));
                return false;
            }
            if pending_doc.is_some() {
                doc_comments.push((index, pending_doc.take()));
            }
            index += 1;
            true
        });
        let halt_map = crate::multibyte::take_halt_offset_map();
        Self::apply_halt_compiler(&mut filtered, &mut doc_comments, halt_map);
        Self {
            tokens: filtered,
            token_doc_comments: doc_comments,
            doc_floor: 0,
            pos: 0,
            depth: 0,
            anon_class_stmts: Vec::new(),
            anon_counter: 0,
            first_class_callable_flag: false,
            new_non_dereferenceable: false,
            pending_extra_constants: Vec::new(),
            source: Vec::new(),
            trusted: false,
            pending_extra_properties: Vec::new(),
            last_set_hook_type: None,
            last_get_hook_by_ref: false,
            last_hook_attrs: HookAttrs::default(),
            pending_closure_doc: None,
            class_name_stack: Vec::new(),
            is_interface_stack: Vec::new(),
            stmt_nesting: 0,
            array_syntax: std::collections::HashMap::default(),
            array_literal_depth: 0,
            in_foreach_target: false,
            last_group_close: usize::MAX,
            stack_nesting: 0,
            speculative: 0,
            saw_yield: false,
        }
    }

    /// Take CG(doc_comment) as of the current token: the latest doc comment
    /// lexed so far that no declaration took yet and no `}` (or namespace
    /// declaration) reset since; the ones before it are dropped with it.
    fn take_doc_comment(&mut self) -> Option<Vec<u8>> {
        let end = self.pos.min(self.tokens.len().checked_sub(1)?);
        // (the scanner resets it as it lexes `}` or a namespace name)
        let mut i = end;
        while !(i == 0
            || (i <= self.doc_floor && i < end)
            || matches!(
                self.tokens.get(i - 1).map(|t| &t.kind),
                Some(TokenKind::CloseBrace | TokenKind::Namespace)
            ))
        {
            i -= 1;
        }
        self.doc_floor = self.doc_floor.max(end);
        // The newest of those attached to tokens i..=end; an older one was
        // overwritten by the newer: dropped too.
        let first = self
            .token_doc_comments
            .partition_point(|&(k, _)| (k as usize) < i);
        let mut found: Option<Vec<u8>> = None;
        for (k, d) in &mut self.token_doc_comments[first..] {
            if *k as usize > end {
                break;
            }
            if let Some(d) = d.take() {
                found = Some(d);
            }
        }
        found
    }

    /// Peek the doc comment at the current token without consuming it.
    fn peek_doc_comment(&self) -> Option<&Vec<u8>> {
        let docs = &self.token_doc_comments;
        let i = docs.partition_point(|&(k, _)| (k as usize) < self.pos);
        docs.get(i)
            .filter(|(k, _)| *k as usize == self.pos)
            .and_then(|(_, d)| d.as_ref())
    }

    /// Attach a doc comment to a freshly-parsed Statement that represents a
    /// function/class/etc. declaration. Other statement kinds keep no doc
    /// comment.
    fn attach_stmt_doc_comment(stmt: &mut Statement, doc: Vec<u8>) {
        match &mut stmt.kind {
            // (one the declaration took later is the newer)
            StmtKind::FunctionDecl { doc_comment, .. }
            | StmtKind::ClassDecl { doc_comment, .. }
                if doc_comment.is_none() =>
            {
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

    /// Advance past the current token, handing over the bytes of an
    /// Identifier / Variable / ConstantString (none for other tokens). The
    /// token keeps an empty payload: a token behind the current one is
    /// never looked at again for its text, except after a rewind, which
    /// only happens in speculate() (where the bytes are copied).
    fn advance_take_bytes(&mut self) -> Vec<u8> {
        let i = self.pos.min(self.tokens.len() - 1);
        let speculative = self.speculative != 0;
        let bytes = match &mut self.tokens[i].kind {
            TokenKind::Identifier(v) | TokenKind::Variable(v) | TokenKind::ConstantString(v) => {
                if speculative {
                    v.clone()
                } else {
                    std::mem::take(v)
                }
            }
            _ => Vec::new(),
        };
        self.advance();
        bytes
    }

    /// Run `f` on tokens the parser may rewind to and parse again.
    fn speculate<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.speculative += 1;
        let r = f(self);
        self.speculative -= 1;
        r
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
        // (as deep a nesting takes as many tokens)
        if self.tokens.len() < YYMAXDEPTH {
            return None;
        }
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
        // php-src's own grammar decides what is a syntax error, and how it
        // is worded (see php_recognizer).
        if !self.source.is_empty() && !self.trusted {
            let shebang = self.source.starts_with(b"#!")
                && self.tokens.first().is_some_and(|t| t.span.start > 0);
            let r = crate::php_recognizer::run(&self.source, crate::short_open_tag(), shebang);
            if let Some(e) = r.error {
                return Err(self.merge_syntax_error(e));
            }
            // Nothing after `__halt_compiler();` is scanned.
            if let Some((_, stop_line)) = r.stop {
                if crate::LEXER_ERROR
                    .with(|e| e.borrow().as_ref().is_some_and(|(_, l)| *l > stop_line))
                {
                    let _ = crate::take_lexer_error();
                }
            }
        }
        let result = self.parse_program();
        match result {
            // The scanner raises its error when the parser fetches that token:
            // it wins over a syntax error found at or after it.
            Err(e) => match self.first_nesting_error() {
                Some((idx, message, line)) if idx <= self.pos => {
                    let mut span = e.span;
                    span.line = line;
                    Err(ParseError { message, span })
                }
                _ => Err(e),
            },
            r => r,
        }
    }

    /// Combine php-src's syntax error with what goro's own scanner finds:
    /// errors php-src raises while scanning (invalid literals / escapes)
    /// win when they come first.
    fn merge_syntax_error(&mut self, e: crate::php_recognizer::SyntaxError) -> ParseError {
        let php = ParseError {
            message: e.message,
            span: Span::new(e.offset as u32, e.offset as u32, e.line),
        };
        if let Some((message, line)) = crate::take_lexer_error() {
            // goro's lexer approximates some syntax errors itself; php-src's
            // grammar is authoritative for those.
            if line <= e.line && !message.starts_with("syntax error") {
                let mut span = php.span;
                span.line = line;
                return ParseError { message, span };
            }
        }
        php
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
        Ok(Program {
            statements,
            has_yield: self.saw_yield,
            rtd_slots: self.anon_counter,
        })
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
                let line = self.span().line;
                // attribute_decl: class_name (T_STATIC | name); a fully
                // qualified name keeps its leading backslash.
                let parts: Vec<Vec<u8>> = match self.peek() {
                    TokenKind::Static => {
                        self.advance();
                        vec![b"static".to_vec()]
                    }
                    k => match Self::name_of(k) {
                        Some(n) => {
                            self.advance();
                            let mut parts: Vec<Vec<u8>> =
                                n.split(|&c| c == b'\\').map(|p| p.to_vec()).collect();
                            if parts.len() > 1 && parts[0].is_empty() {
                                parts.remove(0);
                                parts[0].insert(0, b'\\');
                            }
                            parts
                        }
                        None => return Err(self.unexpected_error()),
                    },
                };
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
        match self.peek() {
            TokenKind::InlineHtml(html) => {
                let html = html.clone();
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
                // A doc comment between the attributes and the declaration
                // is the latest one: it wins (GH-20895).
                let doc = match self.peek() {
                    TokenKind::Function
                    | TokenKind::Class
                    | TokenKind::Abstract
                    | TokenKind::Final
                    | TokenKind::Interface
                    | TokenKind::Trait
                    | TokenKind::Enum
                    | TokenKind::Readonly => self.take_doc_comment().or(doc),
                    _ => doc,
                };
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
                // namespace_declaration_name: identifier | T_NAME_QUALIFIED.
                let mut name_parts = Vec::new();
                match self.peek().clone() {
                    TokenKind::Identifier(n) | TokenKind::NameQualified(n) => {
                        self.advance();
                        name_parts = n.split(|&c| c == b'\\').map(|p| p.to_vec()).collect();
                    }
                    TokenKind::OpenBrace => {}
                    TokenKind::NameRelative(_) | TokenKind::NameFullyQualified(_) => {
                        return Err(self.expecting_error("\"{\""));
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
                        name_parts.push(self.keyword_to_identifier());
                        self.advance();
                    }
                    _ => return Err(self.unexpected_error()),
                }
                if name_parts
                    .first()
                    .is_some_and(|p| p.eq_ignore_ascii_case(b"namespace"))
                {
                    let joined = name_parts.join(&b'\\');
                    return Err(ParseError {
                        message: format!(
                            "Cannot use '{}' as namespace name",
                            String::from_utf8_lossy(&joined)
                        ),
                        span,
                    });
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
                let use_type = match self.peek() {
                    TokenKind::Function => Some(UseKind::Function),
                    TokenKind::Const => Some(UseKind::Constant),
                    _ => None,
                };
                if use_type.is_some() {
                    self.advance();
                }
                let default_kind = use_type.unwrap_or(UseKind::Normal);
                let split = |n: &[u8]| -> Vec<Vec<u8>> {
                    n.split(|&c| c == b'\\').map(|p| p.to_vec()).collect()
                };
                // legacy_namespace_name: T_STRING | T_NAME_QUALIFIED |
                // T_NAME_FULLY_QUALIFIED (the leading separator is dropped).
                let legacy_name = |p: &Self| -> Option<Vec<Vec<u8>>> {
                    match p.peek() {
                        TokenKind::Identifier(n) | TokenKind::NameQualified(n) => Some(split(n)),
                        TokenKind::NameFullyQualified(n) => Some(split(&n[1..])),
                        _ => None,
                    }
                };
                let Some(first_parts) = legacy_name(self) else {
                    return Err(if use_type.is_some() {
                        self.expecting_error(
                            "identifier or fully qualified name or namespaced name",
                        )
                    } else {
                        self.unexpected_error()
                    });
                };
                self.advance();
                // `T_AS T_STRING`
                let parse_alias = |p: &mut Self| -> ParseResult<Option<Vec<u8>>> {
                    if !p.eat(&TokenKind::As) {
                        return Ok(None);
                    }
                    match p.peek().clone() {
                        TokenKind::Identifier(a) => {
                            p.advance();
                            Ok(Some(a))
                        }
                        _ => Err(p.expecting_error("identifier")),
                    }
                };
                let mut items = Vec::new();
                if matches!(self.peek(), TokenKind::Backslash)
                    && matches!(self.peek_at(1), TokenKind::OpenBrace)
                {
                    // Group use: `prefix\{ ... }`.
                    self.advance();
                    self.advance();
                    let mixed = use_type.is_none();
                    loop {
                        let item_kind = match self.peek() {
                            TokenKind::Function if mixed => {
                                self.advance();
                                UseKind::Function
                            }
                            TokenKind::Const if mixed => {
                                self.advance();
                                UseKind::Constant
                            }
                            _ => default_kind,
                        };
                        let name = match self.peek() {
                            TokenKind::Identifier(n) | TokenKind::NameQualified(n) => split(n),
                            _ => {
                                return Err(self.expecting_error(
                                    if mixed && item_kind == UseKind::Normal {
                                        "identifier or namespaced name or \"function\" or \"const\""
                                    } else {
                                        "identifier or namespaced name"
                                    },
                                ));
                            }
                        };
                        self.advance();
                        let alias = parse_alias(self)?;
                        let mut full = first_parts.clone();
                        full.extend(name);
                        items.push(UseItem {
                            name: full,
                            alias,
                            kind: item_kind,
                        });
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                        if matches!(self.peek(), TokenKind::CloseBrace) {
                            break;
                        }
                        if !(matches!(
                            self.peek(),
                            TokenKind::Identifier(_) | TokenKind::NameQualified(_)
                        ) || mixed
                            && matches!(self.peek(), TokenKind::Function | TokenKind::Const))
                        {
                            break;
                        }
                    }
                    if !matches!(self.peek(), TokenKind::CloseBrace) {
                        return Err(self.expecting_error("\"}\""));
                    }
                    self.advance();
                } else {
                    let alias = parse_alias(self)?;
                    items.push(UseItem {
                        name: first_parts,
                        alias,
                        kind: default_kind,
                    });
                    while self.eat(&TokenKind::Comma) {
                        let Some(parts) = legacy_name(self) else {
                            return Err(self.expecting_error(
                                "identifier or fully qualified name or namespaced name",
                            ));
                        };
                        self.advance();
                        let alias = parse_alias(self)?;
                        items.push(UseItem {
                            name: parts,
                            alias,
                            kind: default_kind,
                        });
                    }
                    if !matches!(self.peek(), TokenKind::Semicolon | TokenKind::CloseTag) {
                        return Err(self.expecting_error("\",\" or \";\""));
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
                if matches!(self.peek(), TokenKind::Identifier(name) if name.eq_ignore_ascii_case(b"__halt_compiler"))
                    // Check for () and ;
                    && matches!(self.peek_at(1), TokenKind::OpenParen)
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
                // Check for label: identifier followed by colon (but not ::)
                if matches!(self.peek(), TokenKind::Identifier(_)) {
                    if self.tokens.get(self.pos + 1).map(|t| &t.kind) == Some(&TokenKind::Colon) {
                        // Make sure it's not part of a ternary or other construct
                        // Labels appear at statement level as "label:" on their own
                        let name = self.advance_take_bytes(); // consume identifier
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

    /// The line of the token consumed last.
    fn prev_token_line(&self) -> u32 {
        self.tokens
            .get(self.pos.wrapping_sub(1))
            .map_or(0, |t| t.span.line)
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
        let prev_target = std::mem::replace(&mut self.in_foreach_target, true);
        let first = self.parse_expression();
        self.in_foreach_target = prev_target;
        let first = first?;

        let (key, value, by_ref) = if self.eat(&TokenKind::DoubleArrow) {
            // Key was marked as by-ref - this is an error
            if by_ref {
                return Err(ParseError {
                    message: "Key element cannot be a reference".to_string(),
                    span,
                });
            }
            let by_ref_val = self.eat(&TokenKind::Ampersand);
            let prev_target = std::mem::replace(&mut self.in_foreach_target, true);
            let value = self.parse_expression();
            self.in_foreach_target = prev_target;
            (Some(first), value?, by_ref_val)
        } else {
            (None, first, by_ref)
        };
        if let Some((elems, style)) = self.list_target_style(&value) {
            self.verify_list_assign(elems, style)?;
        }

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
                key: key.map(Box::new),
                value: Box::new(value),
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
                is_const: true,
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

        // (backup_doc_comment after the name: the latest doc comment so far)
        let late_doc = self.take_doc_comment();
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
                doc_comment: late_doc,
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
            let param_line = self.current().span.line;
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
                line: param_line,
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

    /// `C::$m(...)` with a method name computed at run time: the callable
    /// `[C::class, $m]` (ZEND_CALLABLE_CONVERT of the dynamic static call),
    /// or the call itself.
    fn dyn_static_call(
        &mut self,
        class: Expr,
        method: Expr,
        args: Vec<Argument>,
        span: Span,
        is_fcc: bool,
    ) -> Expr {
        if !is_fcc {
            return Expr {
                span,
                kind: ExprKind::DynamicStaticMethodCall {
                    class: Box::new(class),
                    method: Box::new(method),
                    args,
                },
            };
        }
        let cspan = class.span;
        let class = match class.kind {
            ExprKind::Identifier(_) => Expr {
                span: cspan,
                kind: ExprKind::ClassConstAccess {
                    class: Box::new(class),
                    constant: b"class".to_vec(),
                },
            },
            _ => class,
        };
        let elem = |value: Expr| ArrayElement {
            key: None,
            value,
            unpack: false,
            by_ref: false,
        };
        Expr {
            span,
            kind: ExprKind::FirstClassCallable(CallableTarget::Function(Box::new(Expr {
                span,
                kind: ExprKind::Array(vec![elem(class), elem(method)]),
            }))),
        }
    }

    /// `C::$$v` / `C::${e}`: the static property named by `name_expr`, or,
    /// followed by `(`, a call of the method named by that variable's value
    /// (member_name: simple_variable).
    fn dynamic_static_member(
        &mut self,
        class: Expr,
        name_expr: Expr,
        member_span: Span,
    ) -> ParseResult<Expr> {
        if !matches!(self.peek(), TokenKind::OpenParen) {
            return Ok(Expr {
                span: class.span.merge(member_span),
                kind: ExprKind::DynamicStaticPropertyAccess {
                    class: Box::new(class),
                    property: Box::new(name_expr),
                },
            });
        }
        self.advance(); // (
        let args = self.parse_arguments()?;
        let is_fcc = self.first_class_callable_flag;
        let end_span = self.span();
        self.expect(&TokenKind::CloseParen)?;
        let span = name_expr.span;
        let full = class.span.merge(end_span);
        let method = Expr {
            kind: ExprKind::DynamicVariable(Box::new(name_expr)),
            span,
        };
        Ok(self.dyn_static_call(class, method, args, full, is_fcc))
    }

    /// A class_name in a trait use / adaptation (`static` is reserved).
    fn parse_trait_name(&mut self) -> ParseResult<Vec<u8>> {
        if matches!(self.peek(), TokenKind::Static) {
            return Err(self.static_trait_name_error());
        }
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
        Ok(n)
    }

    /// An `identifier` (T_STRING or any semi-reserved word).
    fn parse_member_identifier(&mut self) -> ParseResult<Vec<u8>> {
        match self.peek().clone() {
            TokenKind::Identifier(name) => {
                self.advance();
                Ok(name)
            }
            _ if self.is_semi_reserved_keyword() => {
                let kw = self.keyword_to_identifier();
                self.advance();
                Ok(kw)
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

    /// trait_adaptation: trait_precedence ';' | trait_alias ';'
    fn parse_trait_adaptation(&mut self) -> ParseResult<TraitAdaptation> {
        // trait_method_reference: identifier | class_name '::' identifier
        let is_absolute = matches!(self.peek(), TokenKind::Static)
            || Self::is_compound_name(self.peek())
            || matches!(self.peek_at(1), TokenKind::DoubleColon);
        let (trait_name, method) = if is_absolute {
            let t = self.parse_trait_name()?;
            self.expect(&TokenKind::DoubleColon)?;
            (Some(t), self.parse_member_identifier()?)
        } else {
            (None, self.parse_member_identifier()?)
        };
        let adaptation = if let (Some(t), true) =
            (&trait_name, matches!(self.peek(), TokenKind::Insteadof))
        {
            self.advance();
            let mut instead_of = Vec::new();
            loop {
                instead_of.push(self.parse_trait_name()?);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
            TraitAdaptation::Precedence {
                trait_name: t.clone(),
                method,
                instead_of,
            }
        } else {
            self.expect(&TokenKind::As)?;
            let mut new_visibility = None;
            let mut has_modifier = false;
            let mut new_final = false;
            loop {
                let m = match self.peek() {
                    TokenKind::Public => Some(Visibility::Public),
                    TokenKind::Protected => Some(Visibility::Protected),
                    TokenKind::Private => Some(Visibility::Private),
                    TokenKind::Final => {
                        new_final = true;
                        None
                    }
                    TokenKind::Static => {
                        return Err(ParseError {
                            message: "Cannot use \"static\" as method modifier in trait alias"
                                .into(),
                            span: self.span(),
                        });
                    }
                    TokenKind::Abstract => {
                        return Err(ParseError {
                            message: "Cannot use \"abstract\" as method modifier in trait alias"
                                .into(),
                            span: self.span(),
                        });
                    }
                    TokenKind::Readonly => {
                        return Err(ParseError {
                            message: "Cannot use the readonly modifier on a method".into(),
                            span: self.span(),
                        });
                    }
                    _ => break,
                };
                if m.is_some() {
                    if new_visibility.is_some() {
                        return Err(ParseError {
                            message: "Multiple access type modifiers are not allowed".into(),
                            span: self.span(),
                        });
                    }
                    new_visibility = m;
                }
                has_modifier = true;
                self.advance();
            }
            let new_name = if matches!(self.peek(), TokenKind::Semicolon) && has_modifier {
                None
            } else {
                Some(self.parse_member_identifier()?)
            };
            TraitAdaptation::Alias {
                trait_name,
                method,
                new_name,
                new_visibility,
                new_final,
            }
        };
        self.expect(&TokenKind::Semicolon)?;
        Ok(adaptation)
    }

    /// class_name: T_STATIC | name. Fully qualified names keep their
    /// leading `\\`; relative ones are spelled `namespace\\...`.
    fn parse_class_name_ref(&mut self) -> ParseResult<Vec<u8>> {
        if matches!(self.peek(), TokenKind::Static) {
            self.advance();
            return Ok(b"static".to_vec());
        }
        match Self::name_of(self.peek()) {
            Some(n) => {
                self.advance();
                Ok(n)
            }
            None => Err(self.unexpected_error()),
        }
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
        if let Some(n) = Self::name_of(self.peek()) {
            self.advance();
            return Ok(TypeHint::Simple(n));
        }
        match self.peek().clone() {
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
                message: format!(
                    "syntax error, unexpected {}",
                    self.peek().to_php_unexpected()
                ),
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

        // (backup_doc_comment before `{`: the latest doc comment so far)
        let late_doc = self.take_doc_comment();
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
        let rtd_seq = self.anon_counter;
        self.anon_counter += 1;

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
                doc_comment: late_doc,
                rtd_seq,
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
        // (one after the modifiers is the latest: it wins)
        let member_doc_comment = self.take_doc_comment().or(member_doc_comment);

        match self.peek() {
            TokenKind::Case => {
                // enum case
                let case_line = self.span().line;
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
                    line: case_line,
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
                    if matches!(self.peek(), TokenKind::Static)
                        || Self::name_of(self.peek()).is_some()
                    {
                        traits.push(self.parse_trait_name()?);
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
                        adaptations.push(self.parse_trait_adaptation()?);
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
                if name.eq_ignore_ascii_case(b"class") {
                    return Err(ParseError {
                        message: "A class constant must not be called 'class'; it is reserved for class name fetching".into(),
                        span: self.span(),
                    });
                }

                self.expect(&TokenKind::Assign)?;
                let value = self.parse_expression()?;
                // (backup_doc_comment after each element's value)
                let const_doc = self.take_doc_comment().or(const_doc);

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
                    let extra_doc = self.take_doc_comment();
                    self.pending_extra_constants
                        .push(ClassMember::ClassConstant {
                            name: extra_name,
                            value: extra_value,
                            visibility,
                            is_final,
                            attributes: member_attributes.clone(),
                            type_hint: const_type.clone(),
                            doc_comment: extra_doc,
                            group_continues: true,
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
                    group_continues: false,
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
                                hook_attrs: Box::default(),
                                get_hook_by_ref: false,
                                group_continues: true,
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
                    hook_attrs: Box::new(std::mem::take(&mut self.last_hook_attrs)),
                    get_hook_by_ref: std::mem::take(&mut self.last_get_hook_by_ref),
                    group_continues: false,
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
                                hook_attrs: Box::default(),
                                get_hook_by_ref: false,
                                group_continues: true,
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
                    hook_attrs: Box::new(std::mem::take(&mut self.last_hook_attrs)),
                    get_hook_by_ref: std::mem::take(&mut self.last_get_hook_by_ref),
                    group_continues: false,
                })
            }
        }
    }

    /// Parse property hooks: { get { ... } set($value) { ... } }
    /// Returns (get_hook, set_hook)
    /// zend_compile_property_hooks(): a `set(...)` list without exactly
    /// one parameter.
    #[inline(never)]
    fn set_hook_param_count_error(&self, prop_name: &[u8]) -> ParseError {
        let class_name = self
            .class_name_stack
            .last()
            .map(|n| String::from_utf8_lossy(n).into_owned())
            .unwrap_or_else(|| "?".to_string());
        ParseError {
            message: format!(
                "set hook of property {}::${} must accept exactly one parameters",
                class_name,
                String::from_utf8_lossy(prop_name),
            ),
            span: self.span(),
        }
    }

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
            let mut hook_doc = self.take_doc_comment();
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
            if hook_doc.is_none() {
                hook_doc = self.take_doc_comment();
            }
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

            let hook_line = self.span().line;
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
                self.last_hook_attrs.set_doc(false, hook_doc.take());
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
                self.last_hook_attrs.lines[0] = (hook_line, self.prev_token_line());
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
                self.last_hook_attrs.set_doc(true, hook_doc.take());
                if hook_is_by_ref {
                    // zend_compile_params(): a set hook returns void.
                    let span = self.span();
                    let class_name = self
                        .class_name_stack
                        .last()
                        .map(|n| String::from_utf8_lossy(n).into_owned())
                        .unwrap_or_else(|| "?".to_string());
                    crate::push_compile_deprecated_at(
                        format!(
                            "{}::${}::set(): Returning by reference from a void function is deprecated",
                            class_name,
                            String::from_utf8_lossy(prop_name)
                        ),
                        span.line,
                        span.start,
                    );
                }
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
                        TokenKind::CloseParen
                            if type_hint.is_none() && !got_by_ref && !got_variadic =>
                        {
                            return Err(self.set_hook_param_count_error(prop_name));
                        }
                        _ => {
                            return Err(ParseError {
                                message: "expected parameter name in set hook".into(),
                                span: self.span(),
                            });
                        }
                    };
                    if matches!(self.peek(), TokenKind::Comma) {
                        return Err(self.set_hook_param_count_error(prop_name));
                    }
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
                if param_name == b"this" {
                    return Err(ParseError {
                        message: "Cannot use $this as parameter".into(),
                        span: self.span(),
                    });
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
                self.last_hook_attrs.lines[1] = (hook_line, self.prev_token_line());
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
                // catch_name_list: class_name ('|' class_name)*; a fully
                // qualified name starts with an empty part.
                let type_name: Vec<Vec<u8>> = match Self::name_of(self.peek()) {
                    _ if matches!(self.peek(), TokenKind::Static) => {
                        self.advance();
                        vec![b"static".to_vec()]
                    }
                    Some(n) => {
                        self.advance();
                        n.split(|&c| c == b'\\').map(|p| p.to_vec()).collect()
                    }
                    None => return Err(self.unexpected_error()),
                };
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
                is_const: false,
            },
            span,
        })
    }

    // ---- Expression parsing (Pratt parser) ----

    pub fn parse_expression(&mut self) -> ParseResult<Expr> {
        self.enter_depth()?;
        let result = if self.at_lone_operand() {
            self.parse_primary()
        } else if self.at_postfix_operand() {
            self.with_room(Self::parse_expression_from_postfix)
        } else {
            self.with_room(Self::parse_logical_or_low)
        };
        self.leave_depth();
        result
    }

    /// Whether parse_unary_inner() parses the current token's operand with
    /// parse_postfix() (it is no unary operator): all the precedence
    /// levels then start with it.
    #[inline(always)]
    fn at_postfix_operand(&self) -> bool {
        !matches!(
            self.peek(),
            TokenKind::Minus
                | TokenKind::Plus
                | TokenKind::BooleanNot
                | TokenKind::Tilde
                | TokenKind::Increment
                | TokenKind::Decrement
                | TokenKind::IntCast
                | TokenKind::FloatCast
                | TokenKind::StringCast
                | TokenKind::BoolCast
                | TokenKind::ArrayCast
                | TokenKind::ObjectCast
                | TokenKind::UnsetCast
                | TokenKind::RealCast
                | TokenKind::At
                | TokenKind::Clone
                | TokenKind::Print
                | TokenKind::Throw
        )
    }

    /// parse_logical_or_low() when at_postfix_operand(): the first operand
    /// is parsed at once, then each level, from the tightest, carries on
    /// with it (as the descent through them would on its way back).
    fn parse_expression_from_postfix(&mut self) -> ParseResult<Expr> {
        let e = self.parse_postfix()?;
        if Self::ends_expression(self.peek()) {
            return Ok(e);
        }
        let e = self.pow_rest(e)?;
        let e = self.instanceof_rest(e)?;
        let e = self.binary_rest(e, 1)?;
        let e = self.null_coalesce_rest(e)?;
        let e = self.ternary_rest(e)?;
        let e = self.assignment_inner_rest(e)?;
        let e = self.assignment_rest(e)?;
        let e = self.logical_and_low_rest(e)?;
        let e = self.logical_xor_low_rest(e)?;
        self.logical_or_low_rest(e)
    }

    /// Whether the expression at the current token is that token alone
    /// (a variable, literal or constant name before a token that ends any
    /// expression): every level between parse_expression() and
    /// parse_primary() would hand parse_primary()'s result up unchanged.
    #[inline(always)]
    fn at_lone_operand(&self) -> bool {
        matches!(
            self.peek(),
            TokenKind::Variable(_)
                | TokenKind::LongNumber(_)
                | TokenKind::DoubleNumber(_)
                | TokenKind::ConstantString(_)
                | TokenKind::Identifier(_)
                | TokenKind::True
                | TokenKind::False
                | TokenKind::Null
        ) && Self::ends_expression(self.peek_at(1))
    }

    /// A token no precedence level takes as an operator after an operand.
    #[inline(always)]
    fn ends_expression(kind: &TokenKind) -> bool {
        matches!(
            kind,
            TokenKind::Semicolon
                | TokenKind::Comma
                | TokenKind::CloseParen
                | TokenKind::CloseBracket
                | TokenKind::CloseBrace
                | TokenKind::DoubleArrow
                | TokenKind::Colon
                | TokenKind::CloseTag
        )
    }

    /// Run `f` with room on the native stack (checked at every
    /// STACK_CHECK_EVERY-th nesting level).
    #[inline(always)]
    fn with_room(&mut self, f: fn(&mut Self) -> ParseResult<Expr>) -> ParseResult<Expr> {
        self.stack_nesting += 1;
        let result = if self.stack_nesting % STACK_CHECK_EVERY == 1 {
            self.with_room_checked(f)
        } else {
            f(self)
        };
        self.stack_nesting -= 1;
        result
    }

    #[inline(never)]
    fn with_room_checked(&mut self, f: fn(&mut Self) -> ParseResult<Expr>) -> ParseResult<Expr> {
        let span = self.current().span;
        with_stack(|| f(self)).unwrap_or_else(|| {
            Err(ParseError {
                message: "Maximum nesting depth exceeded".into(),
                span,
            })
        })
    }

    /// Low-precedence 'or' (lower than assignment)
    fn parse_logical_or_low(&mut self) -> ParseResult<Expr> {
        let left = self.parse_logical_xor_low()?;
        self.logical_or_low_rest(left)
    }

    /// parse_logical_or_low() after its first operand.
    #[inline(always)]
    fn logical_or_low_rest(&mut self, mut left: Expr) -> ParseResult<Expr> {
        while matches!(self.peek(), TokenKind::Or) {
            self.advance();
            let right = self.parse_short_circuit_rhs(&left, true, Self::parse_logical_xor_low)?;
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
    #[inline(always)]
    fn parse_logical_xor_low(&mut self) -> ParseResult<Expr> {
        let left = self.parse_logical_and_low()?;
        self.logical_xor_low_rest(left)
    }

    #[inline(always)]
    fn logical_xor_low_rest(&mut self, mut left: Expr) -> ParseResult<Expr> {
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
    #[inline(always)]
    fn parse_logical_and_low(&mut self) -> ParseResult<Expr> {
        let left = self.parse_assignment()?;
        self.logical_and_low_rest(left)
    }

    #[inline(always)]
    fn logical_and_low_rest(&mut self, mut left: Expr) -> ParseResult<Expr> {
        while matches!(self.peek(), TokenKind::And) {
            self.advance();
            let right = self.parse_short_circuit_rhs(&left, false, Self::parse_assignment)?;
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

    #[inline(always)]
    fn parse_assignment(&mut self) -> ParseResult<Expr> {
        let e = self.parse_assignment_inner()?;
        self.assignment_rest(e)
    }

    #[inline(always)]
    fn assignment_rest(&mut self, e: Expr) -> ParseResult<Expr> {
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
        if self.at_lone_operand() {
            return self.parse_primary();
        }
        let left = self.parse_ternary()?;
        self.assignment_inner_rest(left)
    }

    /// parse_assignment_inner() after the operand before the operator.
    fn assignment_inner_rest(&mut self, left: Expr) -> ParseResult<Expr> {
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
        // In PHP's grammar an assignment is `variable '=' expr` (or a
        // `list()`/`[...]` destructuring for plain `=`): anything else before
        // the operator, including a parenthesized expression, is a syntax
        // error at the operator.
        let unexpected_here = |p: &Self| ParseError {
            message: format!("syntax error, unexpected {}", p.peek().to_php_unexpected()),
            span: p.span(),
        };
        if self.last_group_close == self.pos.wrapping_sub(1)
            || !self.is_variable_target(Self::graft_target(&left), compound_op.is_none())
        {
            return Err(unexpected_here(self));
        }
        self.advance();
        let by_ref = compound_op.is_none() && matches!(self.peek(), TokenKind::Ampersand);
        if by_ref {
            // `[$a] =& $b` / `list($a) =& $b`: only `variable '=' '&' ...`.
            if matches!(Self::graft_target(&left).kind, ExprKind::Array(_)) {
                return Err(unexpected_here(self));
            }
            self.advance();
            // `$a = &new A` was removed in PHP 7.
            if matches!(self.peek(), TokenKind::New) {
                return Err(unexpected_here(self));
            }
        }
        let right = self.parse_assignment()?;
        let after_span = self.span();
        // zend_verify_list_assign_target() for a destructuring target.
        if compound_op.is_none() && !by_ref {
            if let Some((elems, style)) = self.list_target_style(&left) {
                self.verify_list_assign(elems, style)?;
            }
        }
        // The operator always binds to the variable right before it, however
        // tightly the operators to its left bind. `$a + !$b = $c` is
        // `$a + !($b = $c)`, `$s ?? $s = ''` is `$s ?? ($s = '')`.
        Self::graft_assignment(left, move |target| {
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

    /// `T_INC variable` / `T_DEC variable`: report a non-variable operand
    /// where bison does. A token that cannot start a `variable` is itself
    /// unexpected (`++1`, `++-$a`); `++$a++` fails at the postfix operator;
    /// a dereferenceable non-variable (`++($a)`, `++A`) at the token after it.
    fn check_prefix_incdec_operand(&self, operand_pos: usize, operand: &Expr) -> ParseResult<()> {
        let unexpected = |pos: usize, expecting: &str| {
            let tok = &self.tokens[pos.min(self.tokens.len() - 1)];
            Err(ParseError {
                message: format!(
                    "syntax error, unexpected {}{}",
                    tok.kind.to_php_unexpected(),
                    expecting
                ),
                span: tok.span,
            })
        };
        if matches!(
            operand.kind,
            ExprKind::UnaryOp {
                op: UnaryOp::PostIncrement | UnaryOp::PostDecrement,
                ..
            }
        ) {
            return unexpected(self.pos - 1, "");
        }
        if self.last_group_close != self.pos.wrapping_sub(1)
            && self.is_variable_target(operand, false)
        {
            return Ok(());
        }
        let starts_variable = matches!(
            self.tokens.get(operand_pos).map(|t| &t.kind),
            Some(
                TokenKind::Variable(_)
                    | TokenKind::VariableVariable(_)
                    | TokenKind::Dollar
                    | TokenKind::DollarOpenBrace
                    | TokenKind::Identifier(_)
                    | TokenKind::Backslash
                    | TokenKind::Namespace
                    | TokenKind::Static
                    | TokenKind::OpenParen
                    | TokenKind::OpenBracket
                    | TokenKind::Array
                    | TokenKind::ConstantString(_)
                    | TokenKind::InterpolatedStringPart(_)
                    | TokenKind::InterpolatedStringEnd(_)
                    | TokenKind::New
            )
        );
        if !starts_variable {
            return unexpected(operand_pos, "");
        }
        unexpected(self.pos, ", expecting \"->\" or \"?->\" or \"{\" or \"[\"")
    }

    /// The operand of `left` that graft_assignment() would assign to.
    fn graft_target(left: &Expr) -> &Expr {
        if Self::is_lvalue(left) {
            return left;
        }
        match &left.kind {
            ExprKind::BinaryOp { right, .. }
            | ExprKind::NullCoalesce { right, .. }
            | ExprKind::Pipe {
                callable: right, ..
            } => Self::graft_target(right),
            ExprKind::UnaryOp {
                op,
                operand,
                prefix: true,
            } if !matches!(op, UnaryOp::PreIncrement | UnaryOp::PreDecrement) => {
                Self::graft_target(operand)
            }
            ExprKind::Cast(_, operand) | ExprKind::Suppress(operand) => Self::graft_target(operand),
            ExprKind::Ternary { if_false, .. } => Self::graft_target(if_false),
            _ => left,
        }
    }

    /// Whether `e` is a `variable` of PHP's grammar (an assignment or
    /// increment target as far as the parser is concerned; the compiler
    /// rejects calls and temporaries in write context). `list` also allows a
    /// `list()` / `[...]` destructuring pattern (not an `array(...)` literal).
    fn is_variable_target(&self, e: &Expr, list: bool) -> bool {
        match &e.kind {
            ExprKind::Variable(_)
            | ExprKind::DynamicVariable(_)
            | ExprKind::ArrayAccess { .. }
            | ExprKind::PropertyAccess { .. }
            | ExprKind::StaticPropertyAccess { .. }
            | ExprKind::DynamicStaticPropertyAccess { .. }
            | ExprKind::MethodCall { .. }
            | ExprKind::StaticMethodCall { .. }
            | ExprKind::DynamicStaticMethodCall { .. }
            | ExprKind::FunctionCall { .. }
            | ExprKind::FirstClassCallable(_) => true,
            ExprKind::Array(_) => {
                list && self.array_syntax.get(&(e.span.start, e.span.end))
                    != Some(&LIST_SYNTAX_LONG)
            }
            _ => false,
        }
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
            ExprKind::Pipe { value, callable } => {
                let callable = Self::graft_assignment(*callable, build)?;
                Expr {
                    span: span.merge(callable.span),
                    kind: ExprKind::Pipe {
                        value,
                        callable: Box::new(callable),
                    },
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

    /// `??` binds tighter than `? :` (and is right-associative).
    fn parse_null_coalesce(&mut self) -> ParseResult<Expr> {
        let left = self.parse_logical_or()?;
        self.null_coalesce_rest(left)
    }

    #[inline(always)]
    fn null_coalesce_rest(&mut self, left: Expr) -> ParseResult<Expr> {
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
        let cond = self.parse_null_coalesce()?;
        self.ternary_rest(cond)
    }

    fn ternary_rest(&mut self, cond: Expr) -> ParseResult<Expr> {
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

    /// The binary operators from `||` down to `*` `/` `%`, by precedence
    /// climbing (one loop instead of a function per level): their level
    /// (1 = loosest) and what they build. All are left-associative; the
    /// comparisons and equalities are non-associative in php-src's grammar,
    /// which reports a chain of them as a syntax error first.
    fn binary_op_level(kind: &TokenKind) -> Option<(u8, BinLevelOp)> {
        use BinLevelOp::*;
        Some(match kind {
            TokenKind::BooleanOr => (1, Bin(BinaryOp::BooleanOr)),
            TokenKind::BooleanAnd => (2, Bin(BinaryOp::BooleanAnd)),
            TokenKind::Pipe => (3, Bin(BinaryOp::BitwiseOr)),
            TokenKind::Caret => (4, Bin(BinaryOp::BitwiseXor)),
            TokenKind::Ampersand => (5, Bin(BinaryOp::BitwiseAnd)),
            TokenKind::Equal => (6, Bin(BinaryOp::Equal)),
            TokenKind::Identical => (6, Bin(BinaryOp::Identical)),
            TokenKind::NotEqual => (6, Bin(BinaryOp::NotEqual)),
            TokenKind::NotIdentical => (6, Bin(BinaryOp::NotIdentical)),
            TokenKind::Less => (7, Bin(BinaryOp::Less)),
            TokenKind::Greater => (7, Bin(BinaryOp::Greater)),
            TokenKind::LessEqual => (7, Bin(BinaryOp::LessEqual)),
            TokenKind::GreaterEqual => (7, Bin(BinaryOp::GreaterEqual)),
            TokenKind::Spaceship => (7, Bin(BinaryOp::Spaceship)),
            // `|>` binds tighter than the comparisons and looser than `.`
            TokenKind::PipeGreater => (8, Pipe),
            // `.` binds looser than the shifts and `+`/`-` (PHP 8)
            TokenKind::Dot => (9, Bin(BinaryOp::Concat)),
            TokenKind::ShiftLeft => (10, Bin(BinaryOp::ShiftLeft)),
            TokenKind::ShiftRight => (10, Bin(BinaryOp::ShiftRight)),
            TokenKind::Plus => (11, Bin(BinaryOp::Add)),
            TokenKind::Minus => (11, Bin(BinaryOp::Sub)),
            TokenKind::Star => (12, Bin(BinaryOp::Mul)),
            TokenKind::Slash => (12, Bin(BinaryOp::Div)),
            TokenKind::Percent => (12, Bin(BinaryOp::Mod)),
            _ => return None,
        })
    }

    /// A binary expression whose operators are all of level `min` or
    /// tighter (see binary_op_level); its operands are `**` / unary
    /// expressions.
    fn parse_binary(&mut self, min: u8) -> ParseResult<Expr> {
        let left = self.parse_instanceof()?;
        self.binary_rest(left, min)
    }

    fn binary_rest(&mut self, mut left: Expr, min: u8) -> ParseResult<Expr> {
        while let Some((level, op)) = Self::binary_op_level(self.peek()) {
            if level < min {
                break;
            }
            self.advance();
            let right = match op {
                // `&&` / `||`: an operand a constant left side decides is
                // never compiled.
                BinLevelOp::Bin(BinaryOp::BooleanOr) => {
                    self.parse_short_circuit_rhs(&left, true, |p| p.parse_binary(level + 1))?
                }
                BinLevelOp::Bin(BinaryOp::BooleanAnd) => {
                    self.parse_short_circuit_rhs(&left, false, |p| p.parse_binary(level + 1))?
                }
                _ => self.parse_binary(level + 1)?,
            };
            let span = left.span.merge(right.span);
            left = Expr {
                span,
                kind: match op {
                    BinLevelOp::Bin(op) => ExprKind::BinaryOp {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    BinLevelOp::Pipe => ExprKind::Pipe {
                        value: Box::new(left),
                        callable: Box::new(right),
                    },
                },
            };
        }
        Ok(left)
    }

    #[inline(always)]
    fn parse_logical_or(&mut self) -> ParseResult<Expr> {
        self.parse_binary(1)
    }

    /// Whether `e` compiles to a constant that decides `&&` (`want` =
    /// false) or `||` (`want` = true) on its own: literals, and such an
    /// operator already decided by its left operand.
    fn decides_short_circuit(e: &Expr, want: bool) -> bool {
        let truthy = match &e.kind {
            ExprKind::Int(n) => *n != 0,
            ExprKind::Float(f) => *f != 0.0,
            ExprKind::String(s) => !(s.is_empty() || s.as_slice() == b"0"),
            ExprKind::True => true,
            ExprKind::False | ExprKind::Null => false,
            ExprKind::BinaryOp { op, left, .. } => {
                let and = matches!(op, BinaryOp::BooleanAnd | BinaryOp::LogicalAnd);
                let or = matches!(op, BinaryOp::BooleanOr | BinaryOp::LogicalOr);
                if (and && Self::decides_short_circuit(left, false))
                    || (or && Self::decides_short_circuit(left, true))
                {
                    or
                } else {
                    return false;
                }
            }
            _ => return false,
        };
        truthy == want
    }

    /// Parse the right operand of `&&`/`||`/`and`/`or`; when `left` decides
    /// the result the operand is never compiled (zend_compile_short_circuiting):
    /// its compile-time diagnostics and anonymous classes are dropped.
    fn parse_short_circuit_rhs(
        &mut self,
        left: &Expr,
        is_or: bool,
        parse: impl FnOnce(&mut Self) -> ParseResult<Expr>,
    ) -> ParseResult<Expr> {
        let start_tok = self.pos;
        let anon_before = self.anon_class_stmts.len();
        let right = parse(self)?;
        if Self::decides_short_circuit(left, is_or) && self.pos > start_tok {
            let start = self.tokens[start_tok].span.start;
            let end = self.tokens[self.pos - 1].span.end.max(start + 1);
            crate::mark_dead_range(start, end);
            for st in self.anon_class_stmts.drain(anon_before..) {
                crate::push_dead_anon_class(st);
            }
        }
        Ok(right)
    }

    /// `instanceof` binds tighter than the binary operators and `!`, looser
    /// than `**`, the casts and the other unary operators; its right side is
    /// a class name reference, not an expression.
    fn parse_instanceof(&mut self) -> ParseResult<Expr> {
        let left = self.parse_pow()?;
        self.instanceof_rest(left)
    }

    #[inline(always)]
    fn instanceof_rest(&mut self, mut left: Expr) -> ParseResult<Expr> {
        while matches!(self.peek(), TokenKind::Instanceof) {
            self.advance();
            let right = self.parse_unary()?;
            left = Expr {
                span: left.span.merge(right.span),
                kind: ExprKind::Instanceof {
                    expr: Box::new(left),
                    class: Box::new(right),
                },
            };
            // (`$a instanceof B ** 2`: the instanceof is the base)
            if self.eat(&TokenKind::Pow) {
                let exp = self.parse_pow()?;
                left = Expr {
                    span: left.span.merge(exp.span),
                    kind: ExprKind::BinaryOp {
                        op: BinaryOp::Pow,
                        left: Box::new(left),
                        right: Box::new(exp),
                    },
                };
            }
        }
        Ok(left)
    }

    fn parse_pow(&mut self) -> ParseResult<Expr> {
        let base = self.parse_unary()?;
        self.pow_rest(base)
    }

    #[inline(always)]
    fn pow_rest(&mut self, base: Expr) -> ParseResult<Expr> {
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
        self.with_room(Self::parse_unary_inner)
    }

    fn parse_unary_inner(&mut self) -> ParseResult<Expr> {
        let span = self.span();
        match self.peek() {
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
                let operand_pos = self.pos;
                let operand = self.parse_unary()?;
                self.check_prefix_incdec_operand(operand_pos, &operand)?;
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
                let operand_pos = self.pos;
                let operand = self.parse_unary()?;
                self.check_prefix_incdec_operand(operand_pos, &operand)?;
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
            // zend_compile rejects it: "The (unset) cast is no longer supported".
            TokenKind::UnsetCast => {
                self.advance();
                let operand = self.parse_pow()?;
                Ok(Expr {
                    span: span.merge(operand.span),
                    kind: ExprKind::Cast(CastType::Unset, Box::new(operand)),
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
                    // (rewound to below for a single parenthesized operand)
                    let args = self.speculate(|p| -> ParseResult<Option<Vec<Argument>>> {
                        p.advance(); // consume '('
                        let call_form = match p.peek() {
                            TokenKind::CloseParen | TokenKind::Ellipsis => true,
                            TokenKind::Variable(_) => false,
                            _ => matches!(p.peek_at(1), TokenKind::Colon),
                        };
                        Ok(if call_form {
                            Some(p.parse_arguments()?)
                        } else {
                            let first = p.parse_expression()?;
                            if p.eat(&TokenKind::Comma) {
                                let mut args = vec![Argument {
                                    name: None,
                                    value: first,
                                    unpack: false,
                                }];
                                args.extend(p.parse_arguments()?);
                                Some(args)
                            } else {
                                None
                            }
                        })
                    })?;
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
                // (parsed again below when an assignment follows)
                let mut operand = self.speculate(|p| p.parse_unary())?;
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
                    method_line: 0,
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
                let mut inner = self.parse_expression()?;
                self.expect(&TokenKind::CloseBrace)?;
                // `${'name'}` names a compiled variable, like `$name`
                // (zend_try_compile_cv); so does a name zend_compile folds to
                // a constant string (`${'na' . 'me'}`).
                fn folded_name(e: &Expr) -> Option<Vec<u8>> {
                    match &e.kind {
                        ExprKind::String(s) => Some(s.clone()),
                        ExprKind::Int(n) => Some(n.to_string().into_bytes()),
                        ExprKind::BinaryOp {
                            left,
                            op: BinaryOp::Concat,
                            right,
                        } => {
                            let mut l = folded_name(left)?;
                            l.extend(folded_name(right)?);
                            Some(l)
                        }
                        _ => None,
                    }
                }
                let folded = match &inner.kind {
                    ExprKind::BinaryOp {
                        op: BinaryOp::Concat,
                        ..
                    } => folded_name(&inner),
                    _ => None,
                };
                if let Some(name) = folded {
                    inner = Expr {
                        kind: ExprKind::String(name),
                        span: inner.span,
                    };
                }
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
        let member = match self.peek() {
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
                let name = name.clone();
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

    /// The elements and closing bracket of a `[...]` literal.
    fn parse_short_array_body(&mut self, span: Span) -> ParseResult<Expr> {
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

    /// Whether a (nested) array literal uses list() syntax.
    fn contains_list_syntax(&self, elems: &[ArrayElement]) -> bool {
        elems.iter().any(|e| match &e.value.kind {
            ExprKind::Array(inner) => {
                self.array_syntax
                    .get(&(e.value.span.start, e.value.span.end))
                    == Some(&LIST_SYNTAX_LIST)
                    || self.contains_list_syntax(inner)
            }
            _ => false,
        })
    }

    /// zend_verify_list_assign_target() for the elements of a
    /// destructuring target written with `style`.
    fn verify_list_assign(&self, elems: &[ArrayElement], style: u8) -> ParseResult<()> {
        for e in elems {
            let v = &e.value;
            match &v.kind {
                ExprKind::Array(inner) => {
                    let st = self
                        .array_syntax
                        .get(&(v.span.start, v.span.end))
                        .copied()
                        .unwrap_or(LIST_SYNTAX_SHORT);
                    if st == LIST_SYNTAX_LONG {
                        return Err(ParseError {
                            message: "Cannot assign to array(), use [] instead".into(),
                            span: v.span,
                        });
                    }
                    if st != style {
                        return Err(ParseError {
                            message: "Cannot mix [] and list()".into(),
                            span: v.span,
                        });
                    }
                    self.verify_list_assign(inner, style)?;
                }
                // An empty slot.
                ExprKind::Null if v.span.start == v.span.end => {}
                _ => {
                    // zend_can_write_to_variable()
                    let mut root = v;
                    while let ExprKind::ArrayAccess { array: o, .. }
                    | ExprKind::PropertyAccess {
                        object: o,
                        nullsafe: false,
                        ..
                    } = &root.kind
                    {
                        root = o;
                    }
                    let writable = matches!(
                        root.kind,
                        ExprKind::Variable(_)
                            | ExprKind::DynamicVariable(_)
                            | ExprKind::PropertyAccess { .. }
                            | ExprKind::StaticPropertyAccess { .. }
                            | ExprKind::DynamicStaticPropertyAccess { .. }
                            | ExprKind::FunctionCall { .. }
                            | ExprKind::MethodCall { .. }
                            | ExprKind::StaticMethodCall { .. }
                            | ExprKind::DynamicStaticMethodCall { .. }
                            | ExprKind::Pipe { .. }
                    ) && !Self::is_short_circuited(root);
                    if !writable {
                        return Err(ParseError {
                            message: "Assignments can only happen to writable values".into(),
                            span: v.span,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// The style of a destructuring target, when `e` is an array literal.
    fn list_target_style<'a>(&self, e: &'a Expr) -> Option<(&'a [ArrayElement], u8)> {
        match &e.kind {
            ExprKind::Array(elems) => Some((
                elems,
                self.array_syntax
                    .get(&(e.span.start, e.span.end))
                    .copied()
                    .unwrap_or(LIST_SYNTAX_SHORT),
            )),
            _ => None,
        }
    }

    /// zend_ast_is_short_circuited()
    fn is_short_circuited(e: &Expr) -> bool {
        match &e.kind {
            ExprKind::PropertyAccess { nullsafe: true, .. }
            | ExprKind::MethodCall { nullsafe: true, .. } => true,
            ExprKind::PropertyAccess { object, .. } | ExprKind::MethodCall { object, .. } => {
                Self::is_short_circuited(object)
            }
            ExprKind::ArrayAccess { array, .. } => Self::is_short_circuited(array),
            ExprKind::StaticPropertyAccess { class, .. }
            | ExprKind::StaticMethodCall { class, .. } => Self::is_short_circuited(class),
            _ => false,
        }
    }

    /// The `new_variable` productions after its first part: `[expr]`,
    /// `->name`, `?->name` and `::$var`.
    fn parse_new_variable_tail(&mut self, mut expr: Expr) -> ParseResult<Expr> {
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
                TokenKind::Arrow | TokenKind::NullsafeArrow => {
                    let nullsafe = matches!(self.peek(), TokenKind::NullsafeArrow);
                    self.advance();
                    // After `->` any label is a T_STRING.
                    let prop = if matches!(self.peek(), TokenKind::Identifier(_))
                        || self.is_semi_reserved_keyword()
                    {
                        let name = match self.peek() {
                            TokenKind::Identifier(n) => n.clone(),
                            _ => self.keyword_to_identifier(),
                        };
                        let sp = self.span();
                        self.advance();
                        Expr {
                            kind: ExprKind::Identifier(name),
                            span: sp,
                        }
                    } else {
                        self.parse_primary()?
                    };
                    let s = expr.span;
                    expr = Expr {
                        kind: ExprKind::PropertyAccess {
                            object: Box::new(expr),
                            property: Box::new(prop),
                            nullsafe,
                        },
                        span: s,
                    };
                }
                TokenKind::DoubleColon => {
                    self.advance();
                    let member = match self.peek().clone() {
                        TokenKind::Variable(name) => {
                            self.advance();
                            name
                        }
                        _ => {
                            return Err(ParseError {
                                message: format!(
                                    "syntax error, unexpected {}, expecting variable or \"$\"",
                                    self.peek().to_php_unexpected()
                                ),
                                span: self.span(),
                            });
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
        Ok(expr)
    }

    fn parse_postfix(&mut self) -> ParseResult<Expr> {
        let mut expr = self.parse_primary()?;
        if std::mem::take(&mut self.new_non_dereferenceable) {
            return Ok(expr);
        }

        loop {
            match self.peek() {
                TokenKind::Increment => {
                    // `variable T_INC`: `($a)++`, `1++`, `$a++++` are errors.
                    if self.last_group_close == self.pos.wrapping_sub(1)
                        || !self.is_variable_target(&expr, false)
                    {
                        return Err(ParseError {
                            message: format!(
                                "syntax error, unexpected {}",
                                self.peek().to_php_unexpected()
                            ),
                            span: self.span(),
                        });
                    }
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
                    // `variable T_INC`: `($a)++`, `1++`, `$a++++` are errors.
                    if self.last_group_close == self.pos.wrapping_sub(1)
                        || !self.is_variable_target(&expr, false)
                    {
                        return Err(ParseError {
                            message: format!(
                                "syntax error, unexpected {}",
                                self.peek().to_php_unexpected()
                            ),
                            span: self.span(),
                        });
                    }
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
                                        method_line: member_span.line,
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
                                let is_fcc = self.first_class_callable_flag;
                                let end_span = self.span();
                                self.expect(&TokenKind::CloseParen)?;
                                // Foo::$method(): the method named by the variable's value
                                let full = expr.span.merge(end_span);
                                let method = Expr {
                                    kind: ExprKind::Variable(name),
                                    span: member_span,
                                };
                                expr = self.dyn_static_call(expr, method, args, full, is_fcc);
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
                                // (rewound to `saved` when not a hook call)
                                match self.speculate(|p| p.parse_prop_hook_call(expr, &lit))? {
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
                            expr = self.dynamic_static_member(expr, name_expr, member_span)?;
                        }
                        TokenKind::Dollar => {
                            // Test::$$$bar / Test::$${expr}: the name is the
                            // value of the simple variable after the `$`.
                            self.advance();
                            let name_expr = self.parse_simple_variable()?;
                            expr = self.dynamic_static_member(expr, name_expr, member_span)?;
                        }
                        TokenKind::VariableVariable(name) => {
                            // Test::$$bar - access static property whose name is in $bar
                            self.advance();
                            let var_expr = Expr {
                                kind: ExprKind::Variable(name),
                                span: member_span,
                            };
                            expr = self.dynamic_static_member(expr, var_expr, member_span)?;
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
                                            method_line: member_span.line,
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
                                let is_fcc = self.first_class_callable_flag;
                                let end_span = self.span();
                                self.expect(&TokenKind::CloseParen)?;
                                let full = expr.span.merge(end_span);
                                expr = self.dyn_static_call(expr, inner, args, full, is_fcc);
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
                    let paren_line = self.span().line;
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
                        // A call of a callable_expr is on the line of its
                        // argument list (the parser's lookahead, GH-8810).
                        let mut span = expr.span.merge(end_span);
                        if !matches!(expr.kind, ExprKind::Identifier(_)) {
                            span.line = paren_line;
                        }
                        Expr {
                            span,
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
            && self.last_group_close != self.pos.wrapping_sub(1)
            && matches!(self.peek(), TokenKind::Assign)
            && matches!(self.peek_at(1), TokenKind::Ampersand)
        {
            self.advance();
            self.advance();
            // `$a = &new A` was removed in PHP 7.
            if matches!(self.peek(), TokenKind::New) {
                return Err(ParseError {
                    message: "syntax error, unexpected token \"new\"".into(),
                    span: self.span(),
                });
            }
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

    /// A primary expression: the commonest (a variable, a literal) here,
    /// the others in parse_primary_other().
    #[inline]
    fn parse_primary(&mut self) -> ParseResult<Expr> {
        let span = self.span();
        let kind = match self.peek() {
            TokenKind::Variable(_) => ExprKind::Variable(self.advance_take_bytes()),
            TokenKind::ConstantString(_) => ExprKind::String(self.advance_take_bytes()),
            TokenKind::LongNumber(n) => {
                let n = *n;
                self.advance();
                ExprKind::Int(n)
            }
            _ => return self.parse_primary_other(span),
        };
        Ok(Expr { kind, span })
    }

    #[inline(never)]
    fn parse_primary_other(&mut self, span: Span) -> ParseResult<Expr> {
        match self.peek() {
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
                            kind: ExprKind::Closure(Box::new(ClosureExpr {
                                is_static,
                                params,
                                use_vars,
                                return_type,
                                body,
                                attributes,
                                end_line,
                                doc_comment: attr_doc,
                                returns_by_ref: by_ref_return,
                            })),
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
                            kind: ExprKind::ArrowFunction(Box::new(ArrowFnExpr {
                                is_static,
                                params,
                                return_type,
                                body: Box::new(body),
                                attributes,
                                end_line,
                                doc_comment: attr_doc,
                                parenthesized: false,
                                returns_by_ref: by_ref_return,
                            })),
                        })
                    }
                    _ => Err(ParseError {
                        message: "expected function or fn after attribute".into(),
                        span: self.span(),
                    }),
                }
            }
            TokenKind::DoubleNumber(n) => {
                let n = *n;
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Float(n),
                    span,
                })
            }
            TokenKind::InterpolatedStringPart(s) => {
                let s = s.clone();
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
                                // "$obj?->prop": a nullsafe property fetch.
                                if matches!(self.peek(), TokenKind::NullsafeArrow) {
                                    if let TokenKind::Identifier(prop_name) =
                                        self.peek_at(1).clone()
                                    {
                                        self.advance();
                                        self.advance();
                                        expr = Expr {
                                            kind: ExprKind::PropertyAccess {
                                                object: Box::new(expr),
                                                property: Box::new(Expr {
                                                    kind: ExprKind::Identifier(prop_name),
                                                    span: var_span,
                                                }),
                                                nullsafe: true,
                                            },
                                            span: var_span,
                                        };
                                    }
                                    break;
                                }
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
            TokenKind::DollarOpenBrace | TokenKind::Dollar => self.parse_simple_variable(),
            TokenKind::VariableVariable(name) => {
                let name = name.clone();
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
            TokenKind::Identifier(_)
            | TokenKind::NameQualified(_)
            | TokenKind::NameFullyQualified(_)
            | TokenKind::NameRelative(_) => {
                let full_name = if matches!(self.peek(), TokenKind::Identifier(_)) {
                    self.advance_take_bytes()
                } else {
                    let name = Self::name_of(self.peek()).unwrap_or_default();
                    self.advance();
                    name
                };
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
                self.last_group_close = self.pos - 1;
                // `(fn() => ...)` keeps its parentheses in AST exports
                // (not as a callee: `(fn() => ...)()`).
                if let ExprKind::ArrowFunction(f) = &mut expr.kind {
                    if !matches!(self.peek(), TokenKind::OpenParen) {
                        f.parenthesized = true;
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
                self.array_literal_depth += 1;
                let r = self.parse_short_array_body(span);
                self.array_literal_depth -= 1;
                let e = r?;
                self.array_syntax
                    .insert((e.span.start, e.span.end), LIST_SYNTAX_SHORT);
                if self.array_literal_depth == 0
                    && !self.in_foreach_target
                    && !matches!(self.peek(), TokenKind::Assign)
                {
                    if let ExprKind::Array(elems) = &e.kind {
                        if self.contains_list_syntax(elems) {
                            return Err(ParseError {
                                message: "Cannot use list() as standalone expression".into(),
                                span: e.span,
                            });
                        }
                    }
                }
                Ok(e)
            }
            TokenKind::Array => {
                self.advance();
                self.expect(&TokenKind::OpenParen)?;
                let mut elements = Vec::new();
                while !matches!(self.peek(), TokenKind::CloseParen | TokenKind::Eof) {
                    // An empty element (`array(, 1)`): grammatical, rejected
                    // by the compiler unless destructuring.
                    if matches!(self.peek(), TokenKind::Comma) {
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
                        self.advance();
                        continue;
                    }
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
                let e = Expr {
                    span: span.merge(end_span),
                    kind: ExprKind::Array(elements),
                };
                self.array_syntax
                    .insert((e.span.start, e.span.end), LIST_SYNTAX_LONG);
                Ok(e)
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
                    TokenKind::Identifier(_)
                    | TokenKind::NameQualified(_)
                    | TokenKind::NameFullyQualified(_)
                    | TokenKind::NameRelative(_) => {
                        let full_name = Self::name_of(self.peek()).unwrap_or_default();
                        self.advance();
                        let class = Expr {
                            kind: ExprKind::Identifier(full_name),
                            span: class_span,
                        };
                        // new_variable: class_name '::' simple_variable
                        if matches!(self.peek(), TokenKind::DoubleColon)
                            && matches!(self.peek_at(1), TokenKind::Variable(_))
                        {
                            self.parse_new_variable_tail(class)?
                        } else {
                            class
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
                        let expr = Expr {
                            kind: ExprKind::Variable(name),
                            span: class_span,
                        };
                        // new_variable: `new $arr[0]`, `new $obj->class`, ...
                        self.parse_new_variable_tail(expr)?
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
                        // Parse optional extends / implements
                        let extends = if self.eat(&TokenKind::Extends) {
                            Some(self.parse_class_name_ref()?)
                        } else {
                            None
                        };
                        let mut implements = Vec::new();
                        if self.eat(&TokenKind::Implements) {
                            loop {
                                implements.push(self.parse_class_name_ref()?);
                                if !self.eat(&TokenKind::Comma) {
                                    break;
                                }
                            }
                        }
                        // Generate unique anonymous class name
                        // CG(rtd_key_counter) starts at 0
                        let rtd_seq = self.anon_counter;
                        let anon_name = format!("__anonymous_class_{}", rtd_seq);
                        self.anon_counter += 1;
                        // Parse class body with class name on stack for error messages
                        self.class_name_stack.push(anon_name.as_bytes().to_vec());
                        let anon_doc = self.take_doc_comment();
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
                                doc_comment: anon_doc,
                                rtd_seq,
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
                    _ => self.parse_primary()?,
                };
                // class_name '::' simple_variable is a new_variable too.
                let class = if matches!(class.kind, ExprKind::Identifier(_))
                    && matches!(self.peek(), TokenKind::DoubleColon)
                {
                    self.parse_new_variable_tail(class)?
                } else {
                    class
                };
                let has_args = matches!(self.peek(), TokenKind::OpenParen);
                let args = if has_args {
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
                self.new_non_dereferenceable = !has_args;
                Ok(Expr {
                    span,
                    kind: ExprKind::New {
                        class: Box::new(class),
                        args,
                    },
                })
            }
            // `cmd` (lexed as `shell_exec(...)`): ZEND_AST_SHELL_EXEC.
            TokenKind::ShellExecMarker => {
                self.advance();
                let call = self.parse_postfix()?;
                Ok(Expr {
                    span: span.merge(call.span),
                    kind: ExprKind::ShellExec(Box::new(call)),
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
                let stashed = self.pending_closure_doc.take();
                self.advance();
                // Optional & for reference return
                let by_ref_return = self.eat(&TokenKind::Ampersand);
                // (backup_doc_comment: the latest doc comment before the
                // parameter list)
                let closure_doc = self.take_doc_comment().or(stashed);
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
                    kind: ExprKind::Closure(Box::new(ClosureExpr {
                        is_static: false,
                        params,
                        use_vars,
                        return_type,
                        body,
                        attributes: Vec::new(),
                        end_line,
                        doc_comment: closure_doc,
                        returns_by_ref: by_ref_return,
                    })),
                })
            }
            TokenKind::Fn => {
                let stashed = self.pending_closure_doc.take();
                self.advance();
                // Optional & for reference return
                let by_ref_return = self.eat(&TokenKind::Ampersand);
                // (backup_doc_comment: the latest doc comment before the
                // parameter list)
                let arrow_doc = self.take_doc_comment().or(stashed);
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
                    kind: ExprKind::ArrowFunction(Box::new(ArrowFnExpr {
                        is_static: false,
                        params,
                        return_type,
                        body: Box::new(body),
                        attributes: Vec::new(),
                        end_line,
                        doc_comment: arrow_doc,
                        parenthesized: false,
                        returns_by_ref: by_ref_return,
                    })),
                })
            }
            TokenKind::Static
                if matches!(
                    self.tokens.get(self.pos + 1).map(|t| &t.kind),
                    Some(TokenKind::Function | TokenKind::Fn)
                ) =>
            {
                let stashed = self.pending_closure_doc.take();
                self.advance(); // static
                if matches!(self.peek(), TokenKind::Fn) {
                    self.advance();
                    let by_ref_return = self.eat(&TokenKind::Ampersand);
                    // (backup_doc_comment: the latest doc comment before the
                    // parameter list)
                    let static_doc = self.take_doc_comment().or(stashed);
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
                        kind: ExprKind::ArrowFunction(Box::new(ArrowFnExpr {
                            is_static: true,
                            params,
                            return_type,
                            body: Box::new(body),
                            attributes: Vec::new(),
                            end_line,
                            doc_comment: static_doc,
                            parenthesized: false,
                            returns_by_ref: by_ref_return,
                        })),
                    })
                } else {
                    self.advance(); // function
                    let by_ref_return = self.eat(&TokenKind::Ampersand);
                    let static_doc = self.take_doc_comment().or(stashed);
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
                        kind: ExprKind::Closure(Box::new(ClosureExpr {
                            is_static: true,
                            params,
                            use_vars,
                            return_type,
                            body,
                            attributes: Vec::new(),
                            end_line,
                            doc_comment: static_doc,
                            returns_by_ref: by_ref_return,
                        })),
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
                self.saw_yield = true;
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
                self.saw_yield = true;
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
            TokenKind::Static => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Identifier(b"static".to_vec()),
                    span,
                })
            }
            // `T_LIST '(' ...` and `T_READONLY argument_list`; other
            // keywords are not names in an expression.
            TokenKind::List | TokenKind::Readonly
                if matches!(self.peek_at(1), TokenKind::OpenParen) =>
            {
                let is_list = matches!(self.peek(), TokenKind::List);
                let full_name = self.keyword_to_identifier();
                self.advance();
                {
                    self.advance();
                    if is_list {
                        // Parse list() as an Array expression (supports keyed and empty slots)
                        self.array_literal_depth += 1;
                        let elements = self.parse_list_elements();
                        self.array_literal_depth -= 1;
                        let elements = elements?;
                        let end_span = self.span();
                        self.expect(&TokenKind::CloseParen)?;
                        let e = Expr {
                            span: span.merge(end_span),
                            kind: ExprKind::Array(elements),
                        };
                        self.array_syntax
                            .insert((e.span.start, e.span.end), LIST_SYNTAX_LIST);
                        // list() only appears as an assignment target.
                        if self.array_literal_depth == 0
                            && !self.in_foreach_target
                            && !matches!(self.peek(), TokenKind::Assign)
                        {
                            return Err(ParseError {
                                message: format!(
                                    "syntax error, unexpected {}, expecting \"=\"",
                                    self.peek().to_php_unexpected()
                                ),
                                span: self.span(),
                            });
                        }
                        Ok(e)
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
                }
            }
            _ => Err(self.unexpected_error()),
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
            // (only a name followed by `:` is looked at)
            let named = self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|t| t.kind == TokenKind::Colon);
            let name = if !named {
                None
            } else {
                let maybe_name = match self.peek() {
                    TokenKind::Identifier(ident) => Some(ident.clone()),
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
                    self.advance(); // identifier/keyword
                    self.advance(); // colon
                    Some(ident)
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
                let cur = self.span();
                // Zero-width: an empty slot, not a literal `null`.
                let span = Span::new(cur.start, cur.start, cur.line);
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

    /// The source is the engine's own code, known to be valid (the PHP
    /// parts of extensions, compiled at startup): it is not checked against
    /// the grammar a second time.
    pub fn trusted(mut self) -> Self {
        self.trusted = true;
        self
    }

    /// `name` (T_STRING | T_NAME_QUALIFIED | T_NAME_FULLY_QUALIFIED |
    /// T_NAME_RELATIVE) in goro's AST spelling: fully qualified names keep
    /// their leading `\\`, relative ones are `namespace\\...`.
    fn name_of(kind: &TokenKind) -> Option<Vec<u8>> {
        match kind {
            TokenKind::Identifier(n)
            | TokenKind::NameQualified(n)
            | TokenKind::NameFullyQualified(n) => Some(n.clone()),
            TokenKind::NameRelative(n) => Some([b"namespace".as_slice(), &n[9..]].concat()),
            _ => None,
        }
    }

    /// Whether `kind` is a T_NAME_* (multi-segment name) token.
    fn is_compound_name(kind: &TokenKind) -> bool {
        matches!(
            kind,
            TokenKind::NameQualified(_)
                | TokenKind::NameFullyQualified(_)
                | TokenKind::NameRelative(_)
        )
    }

    /// Bison's message for an unexpected current token (no expected list).
    fn unexpected_error(&self) -> ParseError {
        ParseError {
            message: format!("syntax error, unexpected {}", self.unexpected_desc()),
            span: self.span(),
        }
    }

    /// Bison's message for an unexpected current token with the expected list.
    fn expecting_error(&self, expecting: &str) -> ParseError {
        ParseError {
            message: format!(
                "syntax error, unexpected {}, expecting {}",
                self.unexpected_desc(),
                expecting
            ),
            span: self.span(),
        }
    }

    /// zend_yytnamerr() for the current token: the token kind followed by
    /// its source text (truncated at a newline / to 30 characters).
    fn unexpected_desc(&self) -> String {
        self.unexpected_desc_at(self.pos)
    }

    fn unexpected_desc_at(&self, idx: usize) -> String {
        let tok = &self.tokens[idx.min(self.tokens.len() - 1)];
        let kind_name = match &tok.kind {
            TokenKind::LongNumber(_) => "integer",
            TokenKind::DoubleNumber(_) => "floating-point number",
            TokenKind::Identifier(_) | TokenKind::True | TokenKind::False | TokenKind::Null => {
                "identifier"
            }
            TokenKind::NameQualified(_) => "namespaced name",
            TokenKind::NameFullyQualified(_) => "fully qualified name",
            TokenKind::NameRelative(_) => "namespace-relative name",
            TokenKind::Variable(_) => "variable",
            TokenKind::ConstantString(_) => "quoted string",
            _ => return tok.kind.to_php_unexpected(),
        };
        let (s, e) = (tok.span.start as usize, tok.span.end as usize);
        let Some(text) = self.source.get(s..e).filter(|t| !t.is_empty()) else {
            return tok.kind.to_php_unexpected();
        };
        let mut text = text;
        if let Some(nl) = text.iter().position(|&c| c == b'\n') {
            text = &text[..nl];
        }
        let mut kind_name = kind_name;
        if kind_name == "quoted string" {
            match text.first() {
                Some(b'"') => kind_name = "double-quoted string",
                Some(b'\'') => kind_name = "single-quoted string",
                _ => {}
            }
        }
        if matches!(text.first(), Some(b'\'' | b'"')) {
            text = &text[1..];
        }
        if matches!(text.last(), Some(b'\'' | b'"')) {
            text = &text[..text.len() - 1];
        }
        if text.len() > 33 {
            format!(
                "{} \"{}...\"",
                kind_name,
                String::from_utf8_lossy(&text[..30])
            )
        } else {
            format!("{} \"{}\"", kind_name, String::from_utf8_lossy(text))
        }
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

    // Collapse a type to its alternatives (`?T` is `T|null`).
    fn alternatives<'a>(t: &'a TypeHint, out: &mut Vec<&'a TypeHint>) {
        match t {
            TypeHint::Union(parts) => parts.iter().for_each(|p| alternatives(p, out)),
            TypeHint::Nullable(inner) => {
                alternatives(inner, out);
                out.push(&NULL_HINT);
            }
            _ => out.push(t),
        }
    }
    static NULL_HINT: TypeHint = TypeHint::Simple(Vec::new());
    fn name(t: &TypeHint) -> Option<String> {
        match t {
            TypeHint::Simple(n) if n.is_empty() => Some("null".into()),
            TypeHint::Simple(n) => Some(String::from_utf8_lossy(n).to_ascii_lowercase()),
            _ => None,
        }
    }
    let (mut prop_alts, mut param_alts) = (Vec::new(), Vec::new());
    alternatives(prop, &mut prop_alts);
    alternatives(param, &mut param_alts);
    // Only builtin scalar types are decided here: class types (and
    // intersections) need the class hierarchy, checked when linking.
    const BUILTIN: &[&str] = &[
        "int", "float", "string", "bool", "false", "true", "null", "array",
    ];
    let names = |alts: &[&TypeHint]| -> Option<Vec<String>> {
        alts.iter()
            .map(|t| name(t).filter(|n| BUILTIN.contains(&n.as_str())))
            .collect()
    };
    let (Some(prop_names), Some(param_names)) = (names(&prop_alts), names(&param_alts)) else {
        return true;
    };
    prop_names.iter().all(|n| {
        param_names.contains(n)
            || ((n == "false" || n == "true") && param_names.iter().any(|p| p == "bool"))
    })
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

/// What a binary operator of parse_binary() builds.
#[derive(Clone, Copy)]
enum BinLevelOp {
    Bin(BinaryOp),
    Pipe,
}
