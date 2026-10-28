//! Recursive-descent parser producing the [`crate::ast`] tree.
//!
//! Syntax errors are worded as PHP 8.5 prints them ("syntax error,
//! unexpected token "x", expecting ..."). The "expecting" lists follow what
//! PHP reports at each point, learnt from the php-src test suite's expected
//! output and from spot checks on PHP 8.5: PHP lists the tokens that may
//! follow only when there are at most four of them.

mod decl;
mod expr;
mod stmt;
pub(crate) mod strings;

pub use bumpalo::Bump as Arena;
/// A list under construction; `into_bump_slice` moves it into the arena at
/// its exact size (growing a vector inside the arena would leave the old
/// buffers behind).
pub(crate) struct BVec<'a, T: Copy> {
    v: Vec<T>,
    arena: &'a Arena,
}

impl<'a, T: Copy> BVec<'a, T> {
    #[inline]
    pub(crate) fn into_bump_slice(self) -> &'a [T] {
        if self.v.is_empty() {
            return &[];
        }
        self.arena.alloc_slice_copy(&self.v)
    }
}

impl<T: Copy> std::ops::Deref for BVec<'_, T> {
    type Target = Vec<T>;
    #[inline]
    fn deref(&self) -> &Vec<T> {
        &self.v
    }
}

impl<T: Copy> std::ops::DerefMut for BVec<'_, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.v
    }
}

use crate::ast::*;
use crate::lexer::{LexError, Lexer, LexerOptions, count_newlines};
use crate::token::{T, TF_ERROR, TF_TAG, Token};

/// The kind of a fatal error found while parsing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// "Parse error: ..." (a `ParseError` exception in `eval()`).
    Parse,
    /// "Fatal error: ..." raised while parsing (for example invalid modifier
    /// combinations), a compile error.
    Compile,
}

/// A fatal error that stops parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub kind: ErrorKind,
    pub message: String,
    pub line: u32,
    pub offset: u32,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let what = match self.kind {
            ErrorKind::Parse => "Parse error",
            ErrorKind::Compile => "Fatal error",
        };
        write!(f, "{}: {} on line {}", what, self.message, self.line)
    }
}

impl std::error::Error for ParseError {}

pub type PResult<T> = Result<T, ParseError>;

/// Parser configuration.
#[derive(Clone, Copy, Debug)]
pub struct ParseOptions {
    pub short_open_tag: bool,
    /// Start in PHP code (for `eval()`), not in inline HTML.
    pub in_scripting: bool,
    /// Stack the parser may use, in bytes, before it reports "memory
    /// exhausted" as PHP does for very deep nesting. The parser needs
    /// roughly 1-4 KiB per nesting level; PHP accepts up to about 10000
    /// levels, so a caller on a large stack can raise this to match.
    pub stack_limit: usize,
}

impl Default for ParseOptions {
    fn default() -> Self {
        ParseOptions {
            short_open_tag: true,
            in_scripting: false,
            stack_limit: DEFAULT_STACK_LIMIT,
        }
    }
}

/// Safe on any thread with the default 2 MiB stack: a few hundred levels of
/// nested calls or arrays, thousands of nested operators. A caller that runs
/// the parser on a larger stack should raise it (PHP 8.5 accepts nesting up
/// to its parser stack of about 10000 entries).
pub const DEFAULT_STACK_LIMIT: usize = 1 << 20;

/// Parses a PHP file. The AST lives in `arena` and borrows from `src`.
pub fn parse<'a>(arena: &'a Arena, src: &'a [u8]) -> PResult<Program<'a>> {
    parse_with(arena, src, ParseOptions::default())
}

/// Parses code given to `eval()` (starts in PHP mode).
pub fn parse_eval<'a>(arena: &'a Arena, src: &'a [u8]) -> PResult<Program<'a>> {
    parse_with(
        arena,
        src,
        ParseOptions {
            short_open_tag: true,
            in_scripting: true,
            ..ParseOptions::default()
        },
    )
}

pub fn parse_with<'a>(arena: &'a Arena, src: &'a [u8], opts: ParseOptions) -> PResult<Program<'a>> {
    let mut p = Parser::new(arena, src, opts);
    p.stack_base = stack_position();
    p.parse_program()
}

/// An address on the current stack.
#[inline(always)]
fn stack_position() -> usize {
    let marker = 0u8;
    std::hint::black_box(&marker) as *const u8 as usize
}

pub(crate) struct Parser<'a> {
    pub(crate) arena: &'a Arena,
    pub(crate) src: &'a [u8],
    pub(crate) toks: Vec<Token>,
    pub(crate) pos: usize,
    lex_error: Option<LexError>,
    /// Doc comments: (start, end) byte offsets, in source order.
    doc_comments: Vec<(u32, u32)>,
    /// Line at end of input.
    eof_line: u32,
    halt_offset: Option<u32>,
    pub(crate) compile_errors: Vec<CompileError>,
    pub(crate) notices: Vec<Notice>,
    /// Nesting depth, to fail cleanly on pathological input.
    depth: u32,
    /// Inside array items, where a nested `list()` needs no `=`.
    pub(crate) list_ok: u32,
    /// Parsing a return type, where `static` is a type.
    pub(crate) allow_static_type: bool,
    /// Doc comments starting before this offset were taken.
    doc_consumed: u32,
    stack_base: usize,
    stack_limit: usize,
    pub(crate) reserved_identifiers: Vec<u32>,
}

/// Maximum nesting of expressions and statements before giving up (PHP
/// reports "memory exhausted" from its parser stack).
/// Observed on PHP 8.5: 7676 nested `~` parse, 17289 nested `[` do not
/// (test Zend/tests/bug64660.phpt). Callers should give the parser a large
/// stack (tens of MB) for such input.
const MAX_DEPTH: u32 = 10000;

impl<'a> Parser<'a> {
    pub(crate) fn new(arena: &'a Arena, src: &'a [u8], opts: ParseOptions) -> Self {
        let lopts = LexerOptions {
            short_open_tag: opts.short_open_tag,
            parse_mode: true,
        };
        let mut lx = if opts.in_scripting {
            Lexer::new_in_scripting(src, lopts)
        } else {
            Lexer::new(src, lopts)
        };
        let mut toks = Vec::with_capacity(src.len() / 5 + 8);
        let mut doc_comments = Vec::new();
        let mut halt_offset = None;
        while let Some(t) = lx.next_token() {
            if lx.error.is_some() {
                break;
            }
            match t.kind {
                T::Whitespace | T::Comment | T::OpenTag => {}
                T::DocComment => doc_comments.push((t.start, t.end)),
                T::CloseTag => toks.push(Token {
                    kind: T::Semicolon,
                    flags: TF_TAG,
                    ..t
                }),
                T::OpenTagWithEcho => toks.push(Token {
                    kind: T::Echo,
                    flags: TF_TAG,
                    ..t
                }),
                _ => toks.push(t),
            }
            if lx.halted() {
                break;
            }
        }
        if lx.halted() {
            halt_offset = toks.last().map(|t| t.end);
        }
        let lex_error = lx.error.take();
        let end = src.len() as u32;
        let eof_line = lx.line();
        let (eof_pos, flags) = match &lex_error {
            Some(e) => (e.offset, TF_ERROR),
            None => (end, 0),
        };
        toks.push(Token {
            kind: T::Eof,
            flags,
            start: eof_pos,
            end: eof_pos,
            line: eof_line,
        });
        Parser {
            arena,
            src,
            toks,
            pos: 0,
            lex_error,
            doc_comments,
            eof_line,
            halt_offset,
            compile_errors: Vec::new(),
            notices: Vec::new(),
            depth: 0,
            list_ok: 0,
            allow_static_type: false,
            doc_consumed: 0,
            stack_base: 0,
            stack_limit: opts.stack_limit,
            reserved_identifiers: Vec::new(),
        }
    }

    fn parse_program(&mut self) -> PResult<Program<'a>> {
        let stmts = self.top_statements(true)?;
        if self.peek_kind() != T::Eof || self.peek().flags & TF_ERROR != 0 {
            return Err(self.unexpected(&[T::Eof]));
        }
        Ok(Program {
            stmts,
            halt_compiler_offset: self.halt_offset,
            compile_errors: std::mem::take(&mut self.compile_errors),
            notices: std::mem::take(&mut self.notices),
            reserved_identifiers: std::mem::take(&mut self.reserved_identifiers),
        })
    }

    // ----- arena -----

    #[inline]
    pub(crate) fn alloc<T>(&self, v: T) -> &'a T {
        self.arena.alloc(v)
    }

    #[inline]
    pub(crate) fn bvec<T: Copy>(&self) -> BVec<'a, T> {
        BVec {
            v: Vec::new(),
            arena: self.arena,
        }
    }

    #[inline]
    pub(crate) fn bytes(&self, v: &[u8]) -> &'a [u8] {
        self.arena.alloc_slice_copy(v)
    }

    // ----- token access -----

    #[inline]
    pub(crate) fn peek(&self) -> &Token {
        &self.toks[self.pos]
    }

    #[inline]
    pub(crate) fn peek_kind(&self) -> T {
        self.toks[self.pos].kind
    }

    #[inline]
    pub(crate) fn peek_at(&self, n: usize) -> T {
        let i = (self.pos + n).min(self.toks.len() - 1);
        self.toks[i].kind
    }

    #[inline]
    pub(crate) fn bump(&mut self) -> Token {
        let t = self.toks[self.pos];
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    #[inline]
    pub(crate) fn at(&self, k: T) -> bool {
        self.peek_kind() == k
    }

    #[inline]
    pub(crate) fn eat(&mut self, k: T) -> bool {
        if self.peek_kind() == k {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Consumes a token of kind `k`, or fails listing `expected`.
    #[inline]
    pub(crate) fn expect(&mut self, k: T, expected: &[T]) -> PResult<Token> {
        if self.peek_kind() == k {
            Ok(self.bump())
        } else {
            Err(self.unexpected(expected))
        }
    }

    /// Consumes `k`; the error lists only `k`.
    #[inline]
    pub(crate) fn expect1(&mut self, k: T) -> PResult<Token> {
        if self.peek_kind() == k {
            Ok(self.bump())
        } else {
            Err(self.unexpected(&[k]))
        }
    }

    #[inline]
    pub(crate) fn text(&self, t: &Token) -> &'a [u8] {
        &self.src[t.start as usize..t.end as usize]
    }

    pub(crate) fn enter(&mut self) -> PResult<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH
            || (self.stack_base != 0
                && self.stack_base.abs_diff(stack_position()) > self.stack_limit)
        {
            let t = *self.peek();
            return Err(ParseError {
                kind: ErrorKind::Parse,
                message: "memory exhausted".into(),
                line: t.line,
                offset: t.start,
            });
        }
        Ok(())
    }

    #[inline]
    pub(crate) fn leave(&mut self) {
        self.depth -= 1;
    }

    // ----- spans -----

    /// Span from token index `start` to the last consumed token.
    #[inline]
    pub(crate) fn span_from(&self, start: usize) -> Span {
        let a = &self.toks[start];
        let end_i = if self.pos > start {
            self.pos - 1
        } else {
            start
        };
        let b = &self.toks[end_i];
        Span {
            start: a.start,
            end: b.end,
            line: a.line,
            end_line: b.line,
        }
    }

    #[inline]
    pub(crate) fn tok_span(&self, t: &Token) -> Span {
        Span {
            start: t.start,
            end: t.end,
            line: t.line,
            end_line: t.line,
        }
    }

    /// Takes the pending doc comment for a declaration whose keyword (or
    /// name) is the token at index `at`. Observed on PHP 8.5 with
    /// getDocComment(): the latest doc comment is pending until a
    /// declaration takes it or a `{` / `}` clears it; `;` does not.
    pub(crate) fn doc_comment_before(&mut self, at: usize) -> Option<&'a [u8]> {
        let pos = self.toks[at].start;
        let idx = self.doc_comments.partition_point(|&(s, _)| s < pos);
        if idx == 0 {
            return None;
        }
        let (s, e) = self.doc_comments[idx - 1];
        if s < self.doc_consumed {
            return None;
        }
        let mut i = at;
        while i > 0 {
            i -= 1;
            let t = &self.toks[i];
            if t.start < e {
                break;
            }
            if matches!(
                t.kind,
                T::LBrace | T::RBrace | T::CurlyOpen | T::DollarOpenCurlyBraces
            ) {
                return None;
            }
        }
        self.doc_consumed = s + 1;
        Some(&self.src[s as usize..e as usize])
    }

    // ----- errors -----

    pub(crate) fn compile_error(&mut self, msg: impl Into<String>, span: Span) {
        self.compile_errors.push(CompileError {
            message: msg.into(),
            line: span.line,
            offset: span.start,
        });
    }

    pub(crate) fn notice(&mut self, kind: NoticeKind, msg: impl Into<String>, line: u32) {
        self.notices.push(Notice {
            kind,
            message: msg.into(),
            line,
        });
    }

    /// A fatal compile error raised while parsing.
    pub(crate) fn fatal(&self, msg: impl Into<String>, span: Span) -> ParseError {
        ParseError {
            kind: ErrorKind::Compile,
            message: msg.into(),
            line: span.line,
            offset: span.start,
        }
    }

    /// A parse error with a custom message at token `t`.
    pub(crate) fn parse_error_at(&self, msg: impl Into<String>, t: &Token) -> ParseError {
        ParseError {
            kind: ErrorKind::Parse,
            message: msg.into(),
            line: self.error_line(t),
            offset: t.start,
        }
    }

    fn error_line(&self, t: &Token) -> u32 {
        if t.kind == T::Eof {
            return self.eof_line;
        }
        if t.flags & TF_TAG != 0 {
            return t.line;
        }
        t.line + count_newlines(self.text(t))
    }

    /// "syntax error, unexpected X[, expecting A or B]" at the current
    /// token. `expected` empty or longer than four: no list (PHP lists at
    /// most four).
    pub(crate) fn unexpected(&self, expected: &[T]) -> ParseError {
        let t = *self.peek();
        if t.flags & TF_ERROR != 0
            && let Some(e) = &self.lex_error
        {
            return ParseError {
                kind: ErrorKind::Parse,
                message: e.message.clone(),
                line: e.line,
                offset: e.offset,
            };
        }
        let mut msg = String::from("syntax error, unexpected ");
        msg.push_str(&self.describe(&t));
        if !expected.is_empty() && expected.len() <= 4 {
            let mut ex: Vec<T> = expected.to_vec();
            ex.sort_by_key(|&k| expected_rank(k));
            ex.dedup();
            msg.push_str(", expecting ");
            for (i, k) in ex.iter().enumerate() {
                if i > 0 {
                    msg.push_str(" or ");
                }
                msg.push_str(&expected_name(*k));
            }
        }
        ParseError {
            kind: ErrorKind::Parse,
            message: msg,
            line: self.error_line(&t),
            offset: t.start,
        }
    }

    /// How PHP names the unexpected token.
    fn describe(&self, t: &Token) -> String {
        let text = self.text(t);
        let quoted =
            |label: &str, s: &[u8]| -> String { format!("{} \"{}\"", label, error_text(s)) };
        match t.kind {
            T::Eof => "end of file".into(),
            T::LNumber => quoted("integer", text),
            T::DNumber => quoted("floating-point number", text),
            T::String => quoted("identifier", text),
            T::NameQualified => quoted("namespaced name", text),
            T::NameFullyQualified => quoted("fully qualified name", text),
            T::NameRelative => quoted("namespace-relative name", text),
            T::Variable => quoted("variable", text),
            T::InlineHtml => quoted("inline html", text),
            T::EncapsedAndWhitespace => {
                let s = text.strip_prefix(b"'").unwrap_or(text);
                quoted("string content", s)
            }
            T::ConstantEncapsedString => {
                let s = if matches!(text.first(), Some(b'b' | b'B')) {
                    &text[1..]
                } else {
                    text
                };
                let label = if s.first() == Some(&b'"') {
                    "double-quoted string"
                } else {
                    "single-quoted string"
                };
                quoted(label, &s[1..s.len().saturating_sub(1).max(1)])
            }
            T::StringVarname => quoted("variable name", text),
            T::NumString => quoted("number", text),
            T::StartHeredoc => quoted("heredoc start", text),
            T::EndHeredoc => quoted("heredoc end", text),
            T::DoubleQuote => "double-quote mark".into(),
            T::BadCharacter => format!("character 0x{:02X}", text.first().copied().unwrap_or(0)),
            T::Semicolon if t.flags & TF_TAG != 0 => "token \";\"".into(),
            T::Echo if t.flags & TF_TAG != 0 => "token \"<?=\"".into(),
            k => format!("token \"{}\"", token_alias(k)),
        }
    }
}

/// The text of a token as shown in an error: up to the first newline; past
/// 33 bytes, the first 30 and "..." (observed on PHP 8.5).
fn error_text(s: &[u8]) -> String {
    let s = match s.iter().position(|&b| b == b'\n' || b == b'\r') {
        Some(i) => &s[..i],
        None => s,
    };
    if s.len() > 33 {
        let mut out = String::from_utf8_lossy(&s[..30]).into_owned();
        out.push_str("...");
        out
    } else {
        String::from_utf8_lossy(s).into_owned()
    }
}

/// The canonical spelling of a keyword or punctuation token.
pub(crate) fn token_alias(k: T) -> &'static str {
    use T::*;
    match k {
        Semicolon => ";",
        Colon => ":",
        Comma => ",",
        Dot => ".",
        LBracket => "[",
        RBracket => "]",
        LParen => "(",
        RParen => ")",
        Pipe => "|",
        Caret => "^",
        Amp | AmpFollowedByVarOrVararg | AmpNotFollowedByVarOrVararg => "&",
        Plus => "+",
        Minus => "-",
        Slash => "/",
        Star => "*",
        Eq => "=",
        Percent => "%",
        Bang => "!",
        Tilde => "~",
        Dollar => "$",
        Lt => "<",
        Gt => ">",
        Question => "?",
        At => "@",
        LBrace => "{",
        RBrace => "}",
        DoubleQuote => "\"",
        Backtick => "`",
        Include => "include",
        IncludeOnce => "include_once",
        Eval => "eval",
        Require => "require",
        RequireOnce => "require_once",
        LogicalOr => "or",
        LogicalXor => "xor",
        LogicalAnd => "and",
        Print => "print",
        Yield => "yield",
        YieldFrom => "yield from",
        Instanceof => "instanceof",
        New => "new",
        Clone => "clone",
        Exit => "exit",
        If => "if",
        Elseif => "elseif",
        Else => "else",
        Endif => "endif",
        Echo => "echo",
        Do => "do",
        While => "while",
        Endwhile => "endwhile",
        For => "for",
        Endfor => "endfor",
        Foreach => "foreach",
        Endforeach => "endforeach",
        Declare => "declare",
        Enddeclare => "enddeclare",
        As => "as",
        Switch => "switch",
        Endswitch => "endswitch",
        Case => "case",
        Default => "default",
        Match => "match",
        Break => "break",
        Continue => "continue",
        Goto => "goto",
        Function => "function",
        Fn => "fn",
        Const => "const",
        Return => "return",
        Try => "try",
        Catch => "catch",
        Finally => "finally",
        Throw => "throw",
        Use => "use",
        Insteadof => "insteadof",
        Global => "global",
        Static => "static",
        Abstract => "abstract",
        Final => "final",
        Private => "private",
        Protected => "protected",
        Public => "public",
        PrivateSet => "private(set)",
        ProtectedSet => "protected(set)",
        PublicSet => "public(set)",
        Readonly => "readonly",
        Var => "var",
        Unset => "unset",
        Isset => "isset",
        Empty => "empty",
        HaltCompiler => "__halt_compiler",
        Class => "class",
        Trait => "trait",
        Interface => "interface",
        Enum => "enum",
        Extends => "extends",
        Implements => "implements",
        Namespace => "namespace",
        List => "list",
        Array => "array",
        Callable => "callable",
        Line => "__LINE__",
        File => "__FILE__",
        Dir => "__DIR__",
        ClassC => "__CLASS__",
        TraitC => "__TRAIT__",
        MethodC => "__METHOD__",
        FuncC => "__FUNCTION__",
        PropertyC => "__PROPERTY__",
        NsC => "__NAMESPACE__",
        Attribute => "#[",
        PlusEqual => "+=",
        MinusEqual => "-=",
        MulEqual => "*=",
        DivEqual => "/=",
        ConcatEqual => ".=",
        ModEqual => "%=",
        AndEqual => "&=",
        OrEqual => "|=",
        XorEqual => "^=",
        SlEqual => "<<=",
        SrEqual => ">>=",
        CoalesceEqual => "??=",
        BooleanOr => "||",
        BooleanAnd => "&&",
        IsEqual => "==",
        IsNotEqual => "!=",
        IsIdentical => "===",
        IsNotIdentical => "!==",
        IsSmallerOrEqual => "<=",
        IsGreaterOrEqual => ">=",
        Spaceship => "<=>",
        Sl => "<<",
        Sr => ">>",
        Inc => "++",
        Dec => "--",
        IntCast => "(int)",
        DoubleCast => "(float)",
        StringCast => "(string)",
        ArrayCast => "(array)",
        ObjectCast => "(object)",
        BoolCast => "(bool)",
        UnsetCast => "(unset)",
        VoidCast => "(void)",
        ObjectOperator => "->",
        NullsafeObjectOperator => "?->",
        DoubleArrow => "=>",
        OpenTag => "<?php",
        OpenTagWithEcho => "<?=",
        CloseTag => "?>",
        StartHeredoc => "heredoc start",
        EndHeredoc => "heredoc end",
        DollarOpenCurlyBraces => "${",
        CurlyOpen => "{$",
        DoubleColon => "::",
        NsSeparator => "\\",
        Ellipsis => "...",
        Coalesce => "??",
        Pow => "**",
        PowEqual => "**=",
        PipeGt => "|>",
        _ => "?",
    }
}

/// How a token is named in an "expecting" list.
fn expected_name(k: T) -> String {
    match k {
        T::Eof => "end of file".into(),
        T::LNumber => "integer".into(),
        T::DNumber => "floating-point number".into(),
        T::String => "identifier".into(),
        T::NameQualified => "namespaced name".into(),
        T::NameFullyQualified => "fully qualified name".into(),
        T::NameRelative => "namespace-relative name".into(),
        T::Variable => "variable".into(),
        T::InlineHtml => "inline html".into(),
        T::EncapsedAndWhitespace => "string content".into(),
        T::ConstantEncapsedString => "quoted string".into(),
        T::StringVarname => "variable name".into(),
        T::NumString => "number".into(),
        T::StartHeredoc => "heredoc start".into(),
        T::EndHeredoc => "heredoc end".into(),
        T::DoubleQuote => "double-quote mark".into(),
        // Test Zend/tests/gh18026.phpt.
        T::AmpNotFollowedByVarOrVararg => "token \"&\"".into(),
        k => format!("\"{}\"", token_alias(k)),
    }
}

/// Order of tokens in an "expecting" list (observed on PHP 8.5): operators
/// first, then named tokens by id, then other punctuation.
fn expected_rank(k: T) -> u32 {
    const EARLY: &[T] = &[
        T::Comma,
        T::Eq,
        T::Question,
        T::Colon,
        T::Pipe,
        T::Caret,
        T::Amp,
        T::Lt,
        T::Gt,
        T::Dot,
        T::Plus,
        T::Minus,
        T::Star,
        T::Slash,
        T::Percent,
        T::Bang,
        T::Tilde,
        T::At,
    ];
    const LATE: &[T] = &[
        T::Semicolon,
        T::LBrace,
        T::RBrace,
        T::LParen,
        T::RParen,
        T::LBracket,
        T::RBracket,
        T::Dollar,
        T::Backtick,
        T::DoubleQuote,
    ];
    if k == T::Eof {
        return 0;
    }
    if let Some(i) = EARLY.iter().position(|&x| x == k) {
        return 1 + i as u32;
    }
    if let Some(i) = LATE.iter().position(|&x| x == k) {
        return 2000 + i as u32;
    }
    1000 + k.id() as u32
}
