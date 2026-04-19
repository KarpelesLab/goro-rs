/// Source location for error reporting
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
    pub line: u32,
}

impl Span {
    pub fn new(start: u32, end: u32, line: u32) -> Self {
        Self { start, end, line }
    }

    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
            line: self.line.min(other.line),
        }
    }
}

/// PHP token types
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals
    LongNumber(i64),
    DoubleNumber(f64),
    ConstantString(Vec<u8>),         // Single-quoted or resolved string
    InterpolatedStringPart(Vec<u8>), // Part of a double-quoted string
    InterpolatedStringEnd(Vec<u8>),  // Last part of a double-quoted string

    // Identifiers and variables
    Identifier(Vec<u8>),       // function names, class names, etc.
    Variable(Vec<u8>),         // $foo (stored without the $)
    VariableVariable(Vec<u8>), // $$foo (stored without the $$)

    // Inline HTML (outside <?php ... ?>)
    InlineHtml(Vec<u8>),

    // Trivia (only emitted when the lexer is in preserve-trivia mode for
    // token_get_all() / PhpToken::tokenize(); the parser never sees these).
    Whitespace(Vec<u8>),
    LineComment(Vec<u8>),
    BlockComment(Vec<u8>),
    DocComment(Vec<u8>),

    // Keywords
    Abstract,
    And,
    Array,
    As,
    Break,
    Callable,
    Case,
    Catch,
    Class,
    Clone,
    Const,
    Continue,
    Declare,
    Default,
    Do,
    Echo,
    Else,
    ElseIf,
    Empty,
    EndDeclare,
    EndFor,
    EndForeach,
    EndIf,
    EndSwitch,
    EndWhile,
    Enum,
    Eval,
    Exit,
    Extends,
    False,
    Final,
    Finally,
    Fn,
    For,
    Foreach,
    Function,
    Global,
    Goto,
    If,
    Implements,
    Include,
    IncludeOnce,
    Instanceof,
    Insteadof,
    Interface,
    Isset,
    List,
    Match,
    Namespace,
    New,
    Null,
    Or,
    Print,
    Private,
    Protected,
    Public,
    Readonly,
    Require,
    RequireOnce,
    Return,
    Static,
    Switch,
    Throw,
    Trait,
    True,
    Try,
    Unset,
    Use,
    Var,
    While,
    Xor,
    Yield,
    YieldFrom,

    // Operators
    Plus,               // +
    Minus,              // -
    Star,               // *
    Slash,              // /
    Percent,            // %
    Pow,                // **
    Dot,                // .
    Ampersand,          // &
    Pipe,               // |
    Caret,              // ^
    Tilde,              // ~
    ShiftLeft,          // <<
    ShiftRight,         // >>
    BooleanAnd,         // &&
    BooleanOr,          // ||
    BooleanNot,         // !
    Assign,             // =
    PlusAssign,         // +=
    MinusAssign,        // -=
    StarAssign,         // *=
    SlashAssign,        // /=
    PercentAssign,      // %=
    PowAssign,          // **=
    DotAssign,          // .=
    AmpersandAssign,    // &=
    PipeAssign,         // |=
    CaretAssign,        // ^=
    ShiftLeftAssign,    // <<=
    ShiftRightAssign,   // >>=
    NullCoalesceAssign, // ??=
    Equal,              // ==
    Identical,          // ===
    NotEqual,           // !=
    NotIdentical,       // !==
    Less,               // <
    Greater,            // >
    LessEqual,          // <=
    GreaterEqual,       // >=
    Spaceship,          // <=>
    NullCoalesce,       // ??
    Increment,          // ++
    Decrement,          // --
    Arrow,              // ->
    NullsafeArrow,      // ?->
    DoubleArrow,        // =>
    DoubleColon,        // ::
    Ellipsis,           // ...
    At,                 // @
    AttributeOpen,      // #[
    PipeGreater,        // |> (pipe operator, PHP 8.5)

    // Delimiters
    OpenParen,    // (
    CloseParen,   // )
    OpenBracket,  // [
    CloseBracket, // ]
    OpenBrace,    // {
    CloseBrace,   // }
    Semicolon,    // ;
    Comma,        // ,
    QuestionMark, // ?
    Colon,        // :
    Backslash,    // \

    // Cast operators
    IntCast,    // (int)
    FloatCast,  // (float)
    StringCast, // (string)
    BoolCast,   // (bool)
    ArrayCast,  // (array)
    ObjectCast, // (object)
    UnsetCast,  // (unset)
    VoidCast,   // (void) - not a valid cast, but recognized for error reporting
    RealCast,   // (real) - removed in PHP 8.0, gives parse error

    // Special
    OpenTag,      // <?php
    OpenTagShort, // <?=
    CloseTag,     // ?>

    // End of file
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

impl TokenKind {
    /// Return a PHP-style name for this token, suitable for error messages
    pub fn to_php_name(&self) -> String {
        match self {
            TokenKind::LongNumber(_) => "integer".into(),
            TokenKind::DoubleNumber(_) => "floating-point number".into(),
            TokenKind::ConstantString(_) => "constant encapsed string".into(),
            TokenKind::InterpolatedStringPart(_) | TokenKind::InterpolatedStringEnd(_) => "encapsed and whitespace".into(),
            TokenKind::Identifier(n) => format!("\"{}\"", String::from_utf8_lossy(n)),
            TokenKind::Variable(n) => format!("\"${}\"", String::from_utf8_lossy(n)),
            TokenKind::VariableVariable(_) => "\"$\"".into(),
            TokenKind::InlineHtml(_) => "inline HTML".into(),
            TokenKind::Whitespace(_) => "whitespace".into(),
            TokenKind::LineComment(_) | TokenKind::BlockComment(_) => "comment".into(),
            TokenKind::DocComment(_) => "doc comment".into(),
            TokenKind::Abstract => "\"abstract\"".into(),
            TokenKind::And => "\"and\"".into(),
            TokenKind::Array => "\"array\"".into(),
            TokenKind::As => "\"as\"".into(),
            TokenKind::Break => "\"break\"".into(),
            TokenKind::Callable => "\"callable\"".into(),
            TokenKind::Case => "\"case\"".into(),
            TokenKind::Catch => "\"catch\"".into(),
            TokenKind::Class => "\"class\"".into(),
            TokenKind::Clone => "\"clone\"".into(),
            TokenKind::Const => "\"const\"".into(),
            TokenKind::Continue => "\"continue\"".into(),
            TokenKind::Default => "\"default\"".into(),
            TokenKind::Do => "\"do\"".into(),
            TokenKind::Echo => "\"echo\"".into(),
            TokenKind::Else => "\"else\"".into(),
            TokenKind::ElseIf => "\"elseif\"".into(),
            TokenKind::Enum => "\"enum\"".into(),
            TokenKind::Exit => "\"exit\"".into(),
            TokenKind::Extends => "\"extends\"".into(),
            TokenKind::False => "\"false\"".into(),
            TokenKind::Final => "\"final\"".into(),
            TokenKind::Finally => "\"finally\"".into(),
            TokenKind::Fn => "\"fn\"".into(),
            TokenKind::For => "\"for\"".into(),
            TokenKind::Foreach => "\"foreach\"".into(),
            TokenKind::Function => "\"function\"".into(),
            TokenKind::Global => "\"global\"".into(),
            TokenKind::Goto => "\"goto\"".into(),
            TokenKind::If => "\"if\"".into(),
            TokenKind::Implements => "\"implements\"".into(),
            TokenKind::Include | TokenKind::IncludeOnce => "\"include\"".into(),
            TokenKind::Instanceof => "\"instanceof\"".into(),
            TokenKind::Insteadof => "\"insteadof\"".into(),
            TokenKind::Interface => "\"interface\"".into(),
            TokenKind::Isset => "\"isset\"".into(),
            TokenKind::List => "\"list\"".into(),
            TokenKind::Match => "\"match\"".into(),
            TokenKind::Namespace => "\"namespace\"".into(),
            TokenKind::New => "\"new\"".into(),
            TokenKind::Null => "\"null\"".into(),
            TokenKind::Or => "\"or\"".into(),
            TokenKind::Print => "\"print\"".into(),
            TokenKind::Private => "\"private\"".into(),
            TokenKind::Protected => "\"protected\"".into(),
            TokenKind::Public => "\"public\"".into(),
            TokenKind::Readonly => "\"readonly\"".into(),
            TokenKind::Require | TokenKind::RequireOnce => "\"require\"".into(),
            TokenKind::Return => "\"return\"".into(),
            TokenKind::Static => "\"static\"".into(),
            TokenKind::Switch => "\"switch\"".into(),
            TokenKind::Throw => "\"throw\"".into(),
            TokenKind::Trait => "\"trait\"".into(),
            TokenKind::True => "\"true\"".into(),
            TokenKind::Try => "\"try\"".into(),
            TokenKind::Unset => "\"unset\"".into(),
            TokenKind::Use => "\"use\"".into(),
            TokenKind::Var => "\"var\"".into(),
            TokenKind::While => "\"while\"".into(),
            TokenKind::Xor => "\"xor\"".into(),
            TokenKind::Yield => "\"yield\"".into(),
            TokenKind::YieldFrom => "\"yield from\"".into(),
            TokenKind::Plus => "\"+\"".into(),
            TokenKind::Minus => "\"-\"".into(),
            TokenKind::Star => "\"*\"".into(),
            TokenKind::Slash => "\"/\"".into(),
            TokenKind::Percent => "\"%\"".into(),
            TokenKind::Dot => "\".\"".into(),
            TokenKind::Ampersand => "\"&\"".into(),
            TokenKind::Pipe => "\"|\"".into(),
            TokenKind::Caret => "\"^\"".into(),
            TokenKind::Tilde => "\"~\"".into(),
            TokenKind::BooleanAnd => "\"&&\"".into(),
            TokenKind::BooleanOr => "\"||\"".into(),
            TokenKind::BooleanNot => "\"!\"".into(),
            TokenKind::Assign => "\"=\"".into(),
            TokenKind::Equal => "\"==\"".into(),
            TokenKind::Identical => "\"===\"".into(),
            TokenKind::NotEqual => "\"!=\"".into(),
            TokenKind::NotIdentical => "\"!==\"".into(),
            TokenKind::Less => "\"<\"".into(),
            TokenKind::Greater => "\">\"".into(),
            TokenKind::LessEqual => "\"<=\"".into(),
            TokenKind::GreaterEqual => "\">=\"".into(),
            TokenKind::NullCoalesce => "\"??\"".into(),
            TokenKind::NullCoalesceAssign => "\"??=\"".into(),
            TokenKind::Increment => "\"++\"".into(),
            TokenKind::Decrement => "\"--\"".into(),
            TokenKind::Arrow => "\"->\"".into(),
            TokenKind::NullsafeArrow => "\"?->\"".into(),
            TokenKind::DoubleArrow => "\"=>\"".into(),
            TokenKind::DoubleColon => "\"::\"".into(),
            TokenKind::Ellipsis => "\"...\"".into(),
            TokenKind::At => "\"@\"".into(),
            TokenKind::OpenParen => "\"(\"".into(),
            TokenKind::CloseParen => "\")\"".into(),
            TokenKind::OpenBracket => "\"[\"".into(),
            TokenKind::CloseBracket => "\"]\"".into(),
            TokenKind::OpenBrace => "\"{\"".into(),
            TokenKind::CloseBrace => "\"}\"".into(),
            TokenKind::Semicolon => "\";\"".into(),
            TokenKind::Comma => "\",\"".into(),
            TokenKind::QuestionMark => "\"?\"".into(),
            TokenKind::Colon => "\":\"".into(),
            TokenKind::Backslash => "\"\\\"".into(),
            TokenKind::IntCast => "\"(int)\"".into(),
            TokenKind::FloatCast => "\"(float)\"".into(),
            TokenKind::StringCast => "\"(string)\"".into(),
            TokenKind::BoolCast => "\"(bool)\"".into(),
            TokenKind::ArrayCast => "\"(array)\"".into(),
            TokenKind::ObjectCast => "\"(object)\"".into(),
            TokenKind::UnsetCast => "\"(unset)\"".into(),
            TokenKind::VoidCast => "\"(void)\"".into(),
            TokenKind::RealCast => "\"(real)\"".into(),
            TokenKind::Eof => "end of file".into(),
            _ => format!("{:?}", self),
        }
    }

    /// Return the "unexpected ..." string for syntax error messages.
    /// PHP uses different prefixes: "token" for keywords/operators, type name for literals,
    /// no prefix for EOF.
    pub fn to_php_unexpected(&self) -> String {
        match self {
            TokenKind::Eof => "end of file".into(),
            TokenKind::LongNumber(n) => format!("integer \"{}\"", n),
            TokenKind::DoubleNumber(n) => format!("floating-point number \"{}\"", n),
            TokenKind::ConstantString(s) => {
                let display = String::from_utf8_lossy(s);
                let truncated: String = display.chars().take(15).collect();
                format!("double-quoted string \"{}\"", truncated)
            },
            TokenKind::Identifier(n) => format!("identifier \"{}\"", String::from_utf8_lossy(n)),
            TokenKind::Variable(n) => format!("variable \"${}\"", String::from_utf8_lossy(n)),
            TokenKind::VariableVariable(_) => "variable \"$\"".into(),
            _ => format!("token {}", self.to_php_name()),
        }
    }
}

/// Map a TokenKind to the corresponding PHP T_* numeric id used by token_get_all()
/// and PhpToken::tokenize(). Returns 0 for single-character tokens (which PHP emits
/// as their raw character rather than an [id, text, line] entry).
pub fn token_kind_to_php_id(kind: &TokenKind) -> i64 {
    match kind {
        TokenKind::InlineHtml(_) => 321, // T_INLINE_HTML
        TokenKind::Whitespace(_) => 394, // T_WHITESPACE
        TokenKind::LineComment(_) => 395, // T_COMMENT
        TokenKind::BlockComment(_) => 395, // T_COMMENT (non-doc block)
        TokenKind::DocComment(_) => 396, // T_DOC_COMMENT
        TokenKind::Variable(_) => 320,   // T_VARIABLE
        TokenKind::LongNumber(_) => 260, // T_LNUMBER
        TokenKind::DoubleNumber(_) => 261, // T_DNUMBER
        TokenKind::ConstantString(_) => 318, // T_CONSTANT_ENCAPSED_STRING
        TokenKind::Identifier(_) => 319, // T_STRING
        TokenKind::InterpolatedStringPart(_) => 322, // T_ENCAPSED_AND_WHITESPACE
        TokenKind::InterpolatedStringEnd(_) => 322,
        TokenKind::OpenTag => 392,        // T_OPEN_TAG
        TokenKind::OpenTagShort => 393,   // T_OPEN_TAG_WITH_ECHO
        TokenKind::CloseTag => 393,       // T_CLOSE_TAG (alias)
        TokenKind::Function => 346,
        TokenKind::If => 324,
        TokenKind::Else => 308,
        TokenKind::ElseIf => 307,
        TokenKind::While => 327,
        TokenKind::For => 329,
        TokenKind::Foreach => 331,
        TokenKind::Return => 348,
        TokenKind::Echo => 323,
        TokenKind::Class => 369,
        TokenKind::New => 309,
        TokenKind::Static => 376,
        TokenKind::Public => 371,
        TokenKind::Protected => 372,
        TokenKind::Private => 373,
        TokenKind::Abstract => 374,
        TokenKind::Final => 375,
        TokenKind::Interface => 367,
        TokenKind::Extends => 364,
        TokenKind::Implements => 365,
        TokenKind::As => 332,
        TokenKind::Try => 337,
        TokenKind::Catch => 338,
        TokenKind::Finally => 339,
        TokenKind::Throw => 341,
        TokenKind::Switch => 326,
        TokenKind::Case => 333,
        TokenKind::Default => 334,
        TokenKind::Break => 335,
        TokenKind::Continue => 336,
        TokenKind::Do => 328,
        TokenKind::Instanceof => 310,
        TokenKind::Trait => 368,
        TokenKind::Namespace => 390,
        TokenKind::Use => 357,
        TokenKind::Include => 262,
        TokenKind::IncludeOnce => 263,
        TokenKind::Require => 264,
        TokenKind::RequireOnce => 265,
        TokenKind::Const => 362,
        TokenKind::Isset => 354,
        TokenKind::Unset => 355,
        TokenKind::Empty => 356,
        TokenKind::Yield => 267,
        TokenKind::YieldFrom => 268,
        TokenKind::Match => 349,
        TokenKind::Enum => 389,
        TokenKind::Fn => 347,
        TokenKind::Print => 266,
        TokenKind::Exit => 305,
        TokenKind::Eval => 323, // T_EVAL (conflicts with T_ECHO in our simplified map)
        TokenKind::Clone => 310,
        TokenKind::List => 363,
        TokenKind::Array => 370,
        TokenKind::Callable => 377,
        TokenKind::Readonly => 383,
        TokenKind::Var => 360,
        TokenKind::Global => 358,
        TokenKind::Goto => 361, // T_GOTO (arbitrary since no conflict)
        TokenKind::Null => 0,
        TokenKind::True => 0,
        TokenKind::False => 0,
        TokenKind::And => 312, // T_LOGICAL_AND
        TokenKind::Or => 311,  // T_LOGICAL_OR
        TokenKind::Xor => 313, // T_LOGICAL_XOR
        TokenKind::Declare => 340,
        TokenKind::BooleanAnd => 291,
        TokenKind::BooleanOr => 290,
        TokenKind::Equal => 292,
        TokenKind::NotEqual => 293,
        TokenKind::Identical => 294,
        TokenKind::NotIdentical => 295,
        TokenKind::LessEqual => 296,
        TokenKind::GreaterEqual => 297,
        TokenKind::Spaceship => 298,
        TokenKind::PlusAssign => 277,
        TokenKind::MinusAssign => 278,
        TokenKind::StarAssign => 279,
        TokenKind::SlashAssign => 280,
        TokenKind::DotAssign => 281,
        TokenKind::PercentAssign => 282,
        TokenKind::AmpersandAssign => 283,
        TokenKind::PipeAssign => 284,
        TokenKind::CaretAssign => 285,
        TokenKind::ShiftLeftAssign => 286,
        TokenKind::ShiftRightAssign => 287,
        TokenKind::NullCoalesceAssign => 288,
        TokenKind::NullCoalesce => 400,
        TokenKind::ShiftLeft => 299,
        TokenKind::ShiftRight => 300,
        TokenKind::Pow => 401,
        TokenKind::PowAssign => 289,
        TokenKind::Arrow => 395, // T_OBJECT_OPERATOR
        TokenKind::NullsafeArrow => 396,
        TokenKind::DoubleArrow => 397,
        TokenKind::DoubleColon => 398,
        TokenKind::Ellipsis => 399,
        TokenKind::Increment => 301,
        TokenKind::Decrement => 302,
        TokenKind::IntCast => 303,
        TokenKind::FloatCast => 304,
        TokenKind::StringCast => 305,
        TokenKind::BoolCast => 316,
        TokenKind::ArrayCast => 306,
        TokenKind::ObjectCast => 315,
        TokenKind::UnsetCast => 317,
        _ => 0,
    }
}

/// Reverse map: given a T_* id, return the constant name (e.g. "T_STRING").
/// Returns None for unknown ids.
pub fn token_id_to_name(id: i64) -> Option<&'static str> {
    let name = match id {
        260 => "T_LNUMBER",
        261 => "T_DNUMBER",
        262 => "T_INCLUDE",
        263 => "T_INCLUDE_ONCE",
        264 => "T_REQUIRE",
        265 => "T_REQUIRE_ONCE",
        266 => "T_PRINT",
        267 => "T_YIELD",
        268 => "T_YIELD_FROM",
        305 => "T_EXIT",
        307 => "T_ELSEIF",
        308 => "T_ELSE",
        309 => "T_NEW",
        310 => "T_INSTANCEOF",
        311 => "T_LOGICAL_OR",
        312 => "T_LOGICAL_AND",
        313 => "T_LOGICAL_XOR",
        318 => "T_CONSTANT_ENCAPSED_STRING",
        319 => "T_STRING",
        320 => "T_VARIABLE",
        321 => "T_INLINE_HTML",
        322 => "T_ENCAPSED_AND_WHITESPACE",
        323 => "T_ECHO",
        324 => "T_IF",
        326 => "T_SWITCH",
        327 => "T_WHILE",
        328 => "T_DO",
        329 => "T_FOR",
        331 => "T_FOREACH",
        332 => "T_AS",
        333 => "T_CASE",
        334 => "T_DEFAULT",
        335 => "T_BREAK",
        336 => "T_CONTINUE",
        337 => "T_TRY",
        338 => "T_CATCH",
        339 => "T_FINALLY",
        340 => "T_DECLARE",
        341 => "T_THROW",
        345 => "T_ECHO",
        346 => "T_FUNCTION",
        347 => "T_FN",
        348 => "T_RETURN",
        354 => "T_ISSET",
        355 => "T_UNSET",
        356 => "T_EMPTY",
        357 => "T_USE",
        358 => "T_GLOBAL",
        360 => "T_VAR",
        361 => "T_GOTO",
        362 => "T_CONST",
        363 => "T_LIST",
        364 => "T_EXTENDS",
        365 => "T_IMPLEMENTS",
        367 => "T_INTERFACE",
        368 => "T_TRAIT",
        369 => "T_CLASS",
        370 => "T_ARRAY",
        371 => "T_PUBLIC",
        372 => "T_PROTECTED",
        373 => "T_PRIVATE",
        374 => "T_ABSTRACT",
        375 => "T_FINAL",
        376 => "T_STATIC",
        377 => "T_CALLABLE",
        383 => "T_READONLY",
        349 => "T_MATCH",
        389 => "T_ENUM",
        277 => "T_PLUS_EQUAL",
        278 => "T_MINUS_EQUAL",
        279 => "T_MUL_EQUAL",
        280 => "T_DIV_EQUAL",
        281 => "T_CONCAT_EQUAL",
        282 => "T_MOD_EQUAL",
        283 => "T_AND_EQUAL",
        284 => "T_OR_EQUAL",
        285 => "T_XOR_EQUAL",
        286 => "T_SL_EQUAL",
        287 => "T_SR_EQUAL",
        288 => "T_COALESCE_EQUAL",
        289 => "T_POW_EQUAL",
        290 => "T_BOOLEAN_OR",
        291 => "T_BOOLEAN_AND",
        292 => "T_IS_EQUAL",
        293 => "T_IS_NOT_EQUAL",
        294 => "T_IS_IDENTICAL",
        295 => "T_IS_NOT_IDENTICAL",
        296 => "T_IS_SMALLER_OR_EQUAL",
        297 => "T_IS_GREATER_OR_EQUAL",
        298 => "T_SPACESHIP",
        299 => "T_SL",
        300 => "T_SR",
        301 => "T_INC",
        302 => "T_DEC",
        303 => "T_INT_CAST",
        304 => "T_DOUBLE_CAST",
        306 => "T_ARRAY_CAST",
        315 => "T_OBJECT_CAST",
        316 => "T_BOOL_CAST",
        317 => "T_UNSET_CAST",
        366 => "T_INSTEADOF",
        378 => "T_HALT_COMPILER",
        380 => "T_LINE",
        381 => "T_FILE",
        382 => "T_DIR",
        384 => "T_CLASS_C",
        385 => "T_TRAIT_C",
        386 => "T_METHOD_C",
        387 => "T_FUNC_C",
        388 => "T_NS_C",
        390 => "T_NAMESPACE",
        391 => "T_NS_SEPARATOR",
        392 => "T_OPEN_TAG",
        393 => "T_OPEN_TAG_WITH_ECHO",
        394 => "T_WHITESPACE",
        395 => "T_COMMENT",
        396 => "T_DOC_COMMENT",
        398 => "T_DOUBLE_COLON",
        399 => "T_ELLIPSIS",
        400 => "T_COALESCE",
        401 => "T_POW",
        402 => "T_AMPERSAND_FOLLOWED_BY_VAR_OR_VARARG",
        403 => "T_AMPERSAND_NOT_FOLLOWED_BY_VAR_OR_VARARG",
        _ => return None,
    };
    Some(name)
}

/// Token ids classified as "ignorable" by PhpToken::isIgnorable():
/// whitespace, comments, open/close tag, inline HTML.
pub fn token_id_is_ignorable(id: i64) -> bool {
    // T_INLINE_HTML(321), T_WHITESPACE(394), T_COMMENT(395),
    // T_DOC_COMMENT(396), T_OPEN_TAG(392), T_OPEN_TAG_WITH_ECHO(393).
    matches!(id, 321 | 392 | 393 | 394 | 395 | 396)
}

/// Map identifier bytes to keyword token kind
pub fn keyword_or_identifier(ident: &[u8]) -> TokenKind {
    // Case-insensitive keyword matching (PHP keywords are case-insensitive)
    let lower: Vec<u8> = ident.iter().map(|b| b.to_ascii_lowercase()).collect();
    match lower.as_slice() {
        b"abstract" => TokenKind::Abstract,
        b"and" => TokenKind::And,
        b"array" => TokenKind::Array,
        b"as" => TokenKind::As,
        b"break" => TokenKind::Break,
        b"callable" => TokenKind::Callable,
        b"case" => TokenKind::Case,
        b"catch" => TokenKind::Catch,
        b"class" => TokenKind::Class,
        b"clone" => TokenKind::Clone,
        b"const" => TokenKind::Const,
        b"continue" => TokenKind::Continue,
        b"declare" => TokenKind::Declare,
        b"default" => TokenKind::Default,
        b"do" => TokenKind::Do,
        b"echo" => TokenKind::Echo,
        b"else" => TokenKind::Else,
        b"elseif" => TokenKind::ElseIf,
        b"empty" => TokenKind::Empty,
        b"enddeclare" => TokenKind::EndDeclare,
        b"endfor" => TokenKind::EndFor,
        b"endforeach" => TokenKind::EndForeach,
        b"endif" => TokenKind::EndIf,
        b"endswitch" => TokenKind::EndSwitch,
        b"endwhile" => TokenKind::EndWhile,
        b"enum" => TokenKind::Enum,
        b"eval" => TokenKind::Eval,
        b"exit" | b"die" => TokenKind::Exit,
        b"extends" => TokenKind::Extends,
        b"false" => TokenKind::False,
        b"final" => TokenKind::Final,
        b"finally" => TokenKind::Finally,
        b"fn" => TokenKind::Fn,
        b"for" => TokenKind::For,
        b"foreach" => TokenKind::Foreach,
        b"function" => TokenKind::Function,
        b"global" => TokenKind::Global,
        b"goto" => TokenKind::Goto,
        b"if" => TokenKind::If,
        b"implements" => TokenKind::Implements,
        b"include" => TokenKind::Include,
        b"include_once" => TokenKind::IncludeOnce,
        b"instanceof" => TokenKind::Instanceof,
        b"insteadof" => TokenKind::Insteadof,
        b"interface" => TokenKind::Interface,
        b"isset" => TokenKind::Isset,
        b"list" => TokenKind::List,
        b"match" => TokenKind::Match,
        b"namespace" => TokenKind::Namespace,
        b"new" => TokenKind::New,
        b"null" => TokenKind::Null,
        b"or" => TokenKind::Or,
        b"print" => TokenKind::Print,
        b"private" => TokenKind::Private,
        b"protected" => TokenKind::Protected,
        b"public" => TokenKind::Public,
        b"readonly" => TokenKind::Readonly,
        b"require" => TokenKind::Require,
        b"require_once" => TokenKind::RequireOnce,
        b"return" => TokenKind::Return,
        b"static" => TokenKind::Static,
        b"switch" => TokenKind::Switch,
        b"throw" => TokenKind::Throw,
        b"trait" => TokenKind::Trait,
        b"true" => TokenKind::True,
        b"try" => TokenKind::Try,
        b"unset" => TokenKind::Unset,
        b"use" => TokenKind::Use,
        b"var" => TokenKind::Var,
        b"while" => TokenKind::While,
        b"xor" => TokenKind::Xor,
        b"yield" => TokenKind::YieldFrom, // handled specially in parser for "yield from"
        _ => TokenKind::Identifier(ident.to_vec()),
    }
}
