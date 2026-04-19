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
///
/// The numeric ids follow the PHP 8.2 grammar ordering (generated by Bison from
/// zend_language_parser.y). All three tables in the codebase must stay in sync:
///   * this function (`token_kind_to_php_id`)
///   * `token_id_to_name` below
///   * `register_tokenizer_constants` in crates/goro-core/src/vm.rs (search `b"T_"`)
pub fn token_kind_to_php_id(kind: &TokenKind) -> i64 {
    match kind {
        // Literals / identifiers
        TokenKind::LongNumber(_) => 260,         // T_LNUMBER
        TokenKind::DoubleNumber(_) => 261,       // T_DNUMBER
        TokenKind::Identifier(_) => 262,         // T_STRING
        TokenKind::Variable(_) => 266,           // T_VARIABLE
        TokenKind::InlineHtml(_) => 267,         // T_INLINE_HTML
        TokenKind::InterpolatedStringPart(_) => 268, // T_ENCAPSED_AND_WHITESPACE
        TokenKind::InterpolatedStringEnd(_) => 268,
        TokenKind::ConstantString(_) => 269,     // T_CONSTANT_ENCAPSED_STRING
        TokenKind::VariableVariable(_) => 266,   // emit as T_VARIABLE

        // Keywords
        TokenKind::Include => 272,               // T_INCLUDE
        TokenKind::IncludeOnce => 273,           // T_INCLUDE_ONCE
        TokenKind::Eval => 274,                  // T_EVAL
        TokenKind::Require => 275,               // T_REQUIRE
        TokenKind::RequireOnce => 276,           // T_REQUIRE_ONCE
        TokenKind::Or => 277,                    // T_LOGICAL_OR
        TokenKind::Xor => 278,                   // T_LOGICAL_XOR
        TokenKind::And => 279,                   // T_LOGICAL_AND
        TokenKind::Print => 280,                 // T_PRINT
        TokenKind::Yield => 281,                 // T_YIELD
        TokenKind::YieldFrom => 282,             // T_YIELD_FROM
        TokenKind::Instanceof => 283,            // T_INSTANCEOF
        TokenKind::New => 284,                   // T_NEW
        TokenKind::Clone => 285,                 // T_CLONE
        TokenKind::Exit => 286,                  // T_EXIT
        TokenKind::If => 287,                    // T_IF
        TokenKind::ElseIf => 288,                // T_ELSEIF
        TokenKind::Else => 289,                  // T_ELSE
        TokenKind::EndIf => 290,                 // T_ENDIF
        TokenKind::Echo => 291,                  // T_ECHO
        TokenKind::Do => 292,                    // T_DO
        TokenKind::While => 293,                 // T_WHILE
        TokenKind::EndWhile => 294,              // T_ENDWHILE
        TokenKind::For => 295,                   // T_FOR
        TokenKind::EndFor => 296,                // T_ENDFOR
        TokenKind::Foreach => 297,               // T_FOREACH
        TokenKind::EndForeach => 298,            // T_ENDFOREACH
        TokenKind::Declare => 299,               // T_DECLARE
        TokenKind::EndDeclare => 300,            // T_ENDDECLARE
        TokenKind::As => 301,                    // T_AS
        TokenKind::Switch => 302,                // T_SWITCH
        TokenKind::EndSwitch => 303,             // T_ENDSWITCH
        TokenKind::Case => 304,                  // T_CASE
        TokenKind::Default => 305,               // T_DEFAULT
        TokenKind::Match => 306,                 // T_MATCH
        TokenKind::Break => 307,                 // T_BREAK
        TokenKind::Continue => 308,              // T_CONTINUE
        TokenKind::Goto => 309,                  // T_GOTO
        TokenKind::Function => 310,              // T_FUNCTION
        TokenKind::Fn => 311,                    // T_FN
        TokenKind::Const => 312,                 // T_CONST
        TokenKind::Return => 313,                // T_RETURN
        TokenKind::Try => 314,                   // T_TRY
        TokenKind::Catch => 315,                 // T_CATCH
        TokenKind::Finally => 316,               // T_FINALLY
        TokenKind::Throw => 317,                 // T_THROW
        TokenKind::Use => 318,                   // T_USE
        TokenKind::Insteadof => 319,             // T_INSTEADOF
        TokenKind::Global => 320,                // T_GLOBAL
        TokenKind::Static => 321,                // T_STATIC
        TokenKind::Abstract => 322,              // T_ABSTRACT
        TokenKind::Final => 323,                 // T_FINAL
        TokenKind::Private => 324,               // T_PRIVATE
        TokenKind::Protected => 325,             // T_PROTECTED
        TokenKind::Public => 326,                // T_PUBLIC
        TokenKind::Readonly => 327,              // T_READONLY
        TokenKind::Var => 328,                   // T_VAR
        TokenKind::Unset => 329,                 // T_UNSET
        TokenKind::Isset => 330,                 // T_ISSET
        TokenKind::Empty => 331,                 // T_EMPTY
        TokenKind::Class => 333,                 // T_CLASS
        TokenKind::Trait => 334,                 // T_TRAIT
        TokenKind::Interface => 335,             // T_INTERFACE
        TokenKind::Enum => 336,                  // T_ENUM
        TokenKind::Extends => 337,               // T_EXTENDS
        TokenKind::Implements => 338,            // T_IMPLEMENTS
        TokenKind::Namespace => 339,             // T_NAMESPACE
        TokenKind::List => 340,                  // T_LIST
        TokenKind::Array => 341,                 // T_ARRAY
        TokenKind::Callable => 342,              // T_CALLABLE

        // Operators / composite tokens
        TokenKind::AttributeOpen => 351,         // T_ATTRIBUTE
        TokenKind::PlusAssign => 352,            // T_PLUS_EQUAL
        TokenKind::MinusAssign => 353,           // T_MINUS_EQUAL
        TokenKind::StarAssign => 354,            // T_MUL_EQUAL
        TokenKind::SlashAssign => 355,           // T_DIV_EQUAL
        TokenKind::DotAssign => 356,             // T_CONCAT_EQUAL
        TokenKind::PercentAssign => 357,         // T_MOD_EQUAL
        TokenKind::AmpersandAssign => 358,       // T_AND_EQUAL
        TokenKind::PipeAssign => 359,            // T_OR_EQUAL
        TokenKind::CaretAssign => 360,           // T_XOR_EQUAL
        TokenKind::ShiftLeftAssign => 361,       // T_SL_EQUAL
        TokenKind::ShiftRightAssign => 362,      // T_SR_EQUAL
        TokenKind::NullCoalesceAssign => 363,    // T_COALESCE_EQUAL
        TokenKind::BooleanOr => 364,             // T_BOOLEAN_OR
        TokenKind::BooleanAnd => 365,            // T_BOOLEAN_AND
        TokenKind::Equal => 366,                 // T_IS_EQUAL
        TokenKind::NotEqual => 367,              // T_IS_NOT_EQUAL
        TokenKind::Identical => 368,             // T_IS_IDENTICAL
        TokenKind::NotIdentical => 369,          // T_IS_NOT_IDENTICAL
        TokenKind::LessEqual => 370,             // T_IS_SMALLER_OR_EQUAL
        TokenKind::GreaterEqual => 371,          // T_IS_GREATER_OR_EQUAL
        TokenKind::Spaceship => 372,             // T_SPACESHIP
        TokenKind::ShiftLeft => 373,             // T_SL
        TokenKind::ShiftRight => 374,            // T_SR
        TokenKind::Increment => 375,             // T_INC
        TokenKind::Decrement => 376,             // T_DEC
        TokenKind::IntCast => 377,               // T_INT_CAST
        TokenKind::FloatCast => 378,             // T_DOUBLE_CAST
        TokenKind::StringCast => 379,            // T_STRING_CAST
        TokenKind::ArrayCast => 380,             // T_ARRAY_CAST
        TokenKind::ObjectCast => 381,            // T_OBJECT_CAST
        TokenKind::BoolCast => 382,              // T_BOOL_CAST
        TokenKind::UnsetCast => 383,             // T_UNSET_CAST
        TokenKind::RealCast => 378,              // (real) is an alias for T_DOUBLE_CAST
        TokenKind::VoidCast => 410,              // T_VOID_CAST (PHP 8.5)
        TokenKind::Arrow => 384,                 // T_OBJECT_OPERATOR
        TokenKind::NullsafeArrow => 385,         // T_NULLSAFE_OBJECT_OPERATOR
        TokenKind::DoubleArrow => 386,           // T_DOUBLE_ARROW
        TokenKind::LineComment(_) => 387,        // T_COMMENT
        TokenKind::BlockComment(_) => 387,       // T_COMMENT (non-doc block)
        TokenKind::DocComment(_) => 388,         // T_DOC_COMMENT
        TokenKind::OpenTag => 389,               // T_OPEN_TAG
        TokenKind::OpenTagShort => 390,          // T_OPEN_TAG_WITH_ECHO
        TokenKind::CloseTag => 391,              // T_CLOSE_TAG
        TokenKind::Whitespace(_) => 392,         // T_WHITESPACE
        TokenKind::DoubleColon => 397,           // T_PAAMAYIM_NEKUDOTAYIM / T_DOUBLE_COLON
        TokenKind::Backslash => 398,             // T_NS_SEPARATOR
        TokenKind::Ellipsis => 399,              // T_ELLIPSIS
        TokenKind::NullCoalesce => 400,          // T_COALESCE
        TokenKind::Pow => 401,                   // T_POW
        TokenKind::PowAssign => 402,             // T_POW_EQUAL
        TokenKind::PipeGreater => 411,           // T_PIPE (PHP 8.5)
        TokenKind::Ampersand => 404,             // T_AMPERSAND_NOT_FOLLOWED_BY_VAR_OR_VARARG

        // true/false/null are plain identifiers (T_STRING) at the token level.
        TokenKind::True | TokenKind::False | TokenKind::Null => 262,

        _ => 0,
    }
}

/// Reverse map: given a T_* id, return the constant name (e.g. "T_STRING").
/// Returns None for unknown ids.
pub fn token_id_to_name(id: i64) -> Option<&'static str> {
    let name = match id {
        260 => "T_LNUMBER",
        261 => "T_DNUMBER",
        262 => "T_STRING",
        263 => "T_NAME_FULLY_QUALIFIED",
        264 => "T_NAME_RELATIVE",
        265 => "T_NAME_QUALIFIED",
        266 => "T_VARIABLE",
        267 => "T_INLINE_HTML",
        268 => "T_ENCAPSED_AND_WHITESPACE",
        269 => "T_CONSTANT_ENCAPSED_STRING",
        270 => "T_STRING_VARNAME",
        271 => "T_NUM_STRING",
        272 => "T_INCLUDE",
        273 => "T_INCLUDE_ONCE",
        274 => "T_EVAL",
        275 => "T_REQUIRE",
        276 => "T_REQUIRE_ONCE",
        277 => "T_LOGICAL_OR",
        278 => "T_LOGICAL_XOR",
        279 => "T_LOGICAL_AND",
        280 => "T_PRINT",
        281 => "T_YIELD",
        282 => "T_YIELD_FROM",
        283 => "T_INSTANCEOF",
        284 => "T_NEW",
        285 => "T_CLONE",
        286 => "T_EXIT",
        287 => "T_IF",
        288 => "T_ELSEIF",
        289 => "T_ELSE",
        290 => "T_ENDIF",
        291 => "T_ECHO",
        292 => "T_DO",
        293 => "T_WHILE",
        294 => "T_ENDWHILE",
        295 => "T_FOR",
        296 => "T_ENDFOR",
        297 => "T_FOREACH",
        298 => "T_ENDFOREACH",
        299 => "T_DECLARE",
        300 => "T_ENDDECLARE",
        301 => "T_AS",
        302 => "T_SWITCH",
        303 => "T_ENDSWITCH",
        304 => "T_CASE",
        305 => "T_DEFAULT",
        306 => "T_MATCH",
        307 => "T_BREAK",
        308 => "T_CONTINUE",
        309 => "T_GOTO",
        310 => "T_FUNCTION",
        311 => "T_FN",
        312 => "T_CONST",
        313 => "T_RETURN",
        314 => "T_TRY",
        315 => "T_CATCH",
        316 => "T_FINALLY",
        317 => "T_THROW",
        318 => "T_USE",
        319 => "T_INSTEADOF",
        320 => "T_GLOBAL",
        321 => "T_STATIC",
        322 => "T_ABSTRACT",
        323 => "T_FINAL",
        324 => "T_PRIVATE",
        325 => "T_PROTECTED",
        326 => "T_PUBLIC",
        327 => "T_READONLY",
        328 => "T_VAR",
        329 => "T_UNSET",
        330 => "T_ISSET",
        331 => "T_EMPTY",
        332 => "T_HALT_COMPILER",
        333 => "T_CLASS",
        334 => "T_TRAIT",
        335 => "T_INTERFACE",
        336 => "T_ENUM",
        337 => "T_EXTENDS",
        338 => "T_IMPLEMENTS",
        339 => "T_NAMESPACE",
        340 => "T_LIST",
        341 => "T_ARRAY",
        342 => "T_CALLABLE",
        343 => "T_LINE",
        344 => "T_FILE",
        345 => "T_DIR",
        346 => "T_CLASS_C",
        347 => "T_TRAIT_C",
        348 => "T_METHOD_C",
        349 => "T_FUNC_C",
        350 => "T_NS_C",
        351 => "T_ATTRIBUTE",
        352 => "T_PLUS_EQUAL",
        353 => "T_MINUS_EQUAL",
        354 => "T_MUL_EQUAL",
        355 => "T_DIV_EQUAL",
        356 => "T_CONCAT_EQUAL",
        357 => "T_MOD_EQUAL",
        358 => "T_AND_EQUAL",
        359 => "T_OR_EQUAL",
        360 => "T_XOR_EQUAL",
        361 => "T_SL_EQUAL",
        362 => "T_SR_EQUAL",
        363 => "T_COALESCE_EQUAL",
        364 => "T_BOOLEAN_OR",
        365 => "T_BOOLEAN_AND",
        366 => "T_IS_EQUAL",
        367 => "T_IS_NOT_EQUAL",
        368 => "T_IS_IDENTICAL",
        369 => "T_IS_NOT_IDENTICAL",
        370 => "T_IS_SMALLER_OR_EQUAL",
        371 => "T_IS_GREATER_OR_EQUAL",
        372 => "T_SPACESHIP",
        373 => "T_SL",
        374 => "T_SR",
        375 => "T_INC",
        376 => "T_DEC",
        377 => "T_INT_CAST",
        378 => "T_DOUBLE_CAST",
        379 => "T_STRING_CAST",
        380 => "T_ARRAY_CAST",
        381 => "T_OBJECT_CAST",
        382 => "T_BOOL_CAST",
        383 => "T_UNSET_CAST",
        384 => "T_OBJECT_OPERATOR",
        385 => "T_NULLSAFE_OBJECT_OPERATOR",
        386 => "T_DOUBLE_ARROW",
        387 => "T_COMMENT",
        388 => "T_DOC_COMMENT",
        389 => "T_OPEN_TAG",
        390 => "T_OPEN_TAG_WITH_ECHO",
        391 => "T_CLOSE_TAG",
        392 => "T_WHITESPACE",
        393 => "T_START_HEREDOC",
        394 => "T_END_HEREDOC",
        395 => "T_DOLLAR_OPEN_CURLY_BRACES",
        396 => "T_CURLY_OPEN",
        // PHP's token_name() returns T_DOUBLE_COLON for T_PAAMAYIM_NEKUDOTAYIM
        // (they share the same id).
        397 => "T_DOUBLE_COLON",
        398 => "T_NS_SEPARATOR",
        399 => "T_ELLIPSIS",
        400 => "T_COALESCE",
        401 => "T_POW",
        402 => "T_POW_EQUAL",
        403 => "T_AMPERSAND_FOLLOWED_BY_VAR_OR_VARARG",
        404 => "T_AMPERSAND_NOT_FOLLOWED_BY_VAR_OR_VARARG",
        405 => "T_BAD_CHARACTER",
        406 => "T_PRIVATE_SET",
        407 => "T_PROTECTED_SET",
        408 => "T_PUBLIC_SET",
        409 => "T_PROPERTY_C",
        410 => "T_VOID_CAST",
        411 => "T_PIPE",
        _ => return None,
    };
    Some(name)
}

/// Token ids classified as "ignorable" by PhpToken::isIgnorable():
/// whitespace, comments, open/close tag, inline HTML.
pub fn token_id_is_ignorable(id: i64) -> bool {
    // T_INLINE_HTML(267), T_COMMENT(387), T_DOC_COMMENT(388),
    // T_OPEN_TAG(389), T_OPEN_TAG_WITH_ECHO(390), T_CLOSE_TAG(391),
    // T_WHITESPACE(392).
    matches!(id, 267 | 387 | 388 | 389 | 390 | 391 | 392)
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
