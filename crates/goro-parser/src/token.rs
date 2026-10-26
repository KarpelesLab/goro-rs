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
    Identifier(Vec<u8>), // function names, class names, etc.
    /// T_NAME_QUALIFIED: `Foo\Bar` (the full text).
    NameQualified(Vec<u8>),
    /// T_NAME_FULLY_QUALIFIED: `\Foo\Bar` (the full text, leading `\` included).
    NameFullyQualified(Vec<u8>),
    /// T_NAME_RELATIVE: `namespace\Foo` (the full text as written).
    NameRelative(Vec<u8>),
    Variable(Vec<u8>),         // $foo (stored without the $)
    VariableVariable(Vec<u8>), // $$foo (stored without the $$)
    DollarOpenBrace,           // `${` starting a `${expr}` variable-variable
    Dollar,                    // `$` before another simple variable (`$$$a`, `$${e}`)
    InterpExprStart,           // `{$` opening a braced expression inside a string
    InterpExprEnd,             // the `}` closing it

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
    /// Starts the `shell_exec(...)` call a backtick literal is lexed as.
    ShellExecMarker,
    AttributeOpen, // #[
    PipeGreater,   // |> (pipe operator, PHP 8.5)

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
            TokenKind::InterpolatedStringPart(_) | TokenKind::InterpolatedStringEnd(_) => {
                "encapsed and whitespace".into()
            }
            TokenKind::Identifier(n) => format!("\"{}\"", String::from_utf8_lossy(n)),
            TokenKind::Variable(n) => format!("\"${}\"", String::from_utf8_lossy(n)),
            TokenKind::NameQualified(n)
            | TokenKind::NameFullyQualified(n)
            | TokenKind::NameRelative(n) => format!("\"{}\"", String::from_utf8_lossy(n)),
            TokenKind::VariableVariable(_) => "\"$\"".into(),
            TokenKind::DollarOpenBrace | TokenKind::Dollar => "\"$\"".into(),
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
            TokenKind::Pow => "\"**\"".into(),
            TokenKind::ShiftLeft => "\"<<\"".into(),
            TokenKind::ShiftRight => "\">>\"".into(),
            TokenKind::PlusAssign => "\"+=\"".into(),
            TokenKind::MinusAssign => "\"-=\"".into(),
            TokenKind::StarAssign => "\"*=\"".into(),
            TokenKind::SlashAssign => "\"/=\"".into(),
            TokenKind::PercentAssign => "\"%=\"".into(),
            TokenKind::PowAssign => "\"**=\"".into(),
            TokenKind::DotAssign => "\".=\"".into(),
            TokenKind::AmpersandAssign => "\"&=\"".into(),
            TokenKind::PipeAssign => "\"|=\"".into(),
            TokenKind::CaretAssign => "\"^=\"".into(),
            TokenKind::ShiftLeftAssign => "\"<<=\"".into(),
            TokenKind::ShiftRightAssign => "\">>=\"".into(),
            TokenKind::Spaceship => "\"<=>\"".into(),
            TokenKind::AttributeOpen => "\"#[\"".into(),
            TokenKind::PipeGreater => "\"|>\"".into(),
            // zendlex() hands `?>` to the parser as an implicit `;`.
            TokenKind::CloseTag => "\";\"".into(),
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
            }
            TokenKind::Identifier(n) => format!("identifier \"{}\"", String::from_utf8_lossy(n)),
            TokenKind::NameQualified(n) => {
                format!("namespaced name \"{}\"", String::from_utf8_lossy(n))
            }
            TokenKind::NameFullyQualified(n) => {
                format!("fully qualified name \"{}\"", String::from_utf8_lossy(n))
            }
            TokenKind::NameRelative(n) => {
                format!("namespace-relative name \"{}\"", String::from_utf8_lossy(n))
            }
            TokenKind::Variable(n) => format!("variable \"${}\"", String::from_utf8_lossy(n)),
            TokenKind::VariableVariable(_) => "variable \"$\"".into(),
            _ => format!("token {}", self.to_php_name()),
        }
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
        327 => "T_PRIVATE_SET",
        328 => "T_PROTECTED_SET",
        329 => "T_PUBLIC_SET",
        330 => "T_READONLY",
        331 => "T_VAR",
        332 => "T_UNSET",
        333 => "T_ISSET",
        334 => "T_EMPTY",
        335 => "T_HALT_COMPILER",
        336 => "T_CLASS",
        337 => "T_TRAIT",
        338 => "T_INTERFACE",
        339 => "T_ENUM",
        340 => "T_EXTENDS",
        341 => "T_IMPLEMENTS",
        342 => "T_NAMESPACE",
        343 => "T_LIST",
        344 => "T_ARRAY",
        345 => "T_CALLABLE",
        346 => "T_LINE",
        347 => "T_FILE",
        348 => "T_DIR",
        349 => "T_CLASS_C",
        350 => "T_TRAIT_C",
        351 => "T_METHOD_C",
        352 => "T_FUNC_C",
        353 => "T_PROPERTY_C",
        354 => "T_NS_C",
        355 => "T_ATTRIBUTE",
        356 => "T_PLUS_EQUAL",
        357 => "T_MINUS_EQUAL",
        358 => "T_MUL_EQUAL",
        359 => "T_DIV_EQUAL",
        360 => "T_CONCAT_EQUAL",
        361 => "T_MOD_EQUAL",
        362 => "T_AND_EQUAL",
        363 => "T_OR_EQUAL",
        364 => "T_XOR_EQUAL",
        365 => "T_SL_EQUAL",
        366 => "T_SR_EQUAL",
        367 => "T_COALESCE_EQUAL",
        368 => "T_BOOLEAN_OR",
        369 => "T_BOOLEAN_AND",
        370 => "T_IS_EQUAL",
        371 => "T_IS_NOT_EQUAL",
        372 => "T_IS_IDENTICAL",
        373 => "T_IS_NOT_IDENTICAL",
        374 => "T_IS_SMALLER_OR_EQUAL",
        375 => "T_IS_GREATER_OR_EQUAL",
        376 => "T_SPACESHIP",
        377 => "T_SL",
        378 => "T_SR",
        379 => "T_INC",
        380 => "T_DEC",
        381 => "T_INT_CAST",
        382 => "T_DOUBLE_CAST",
        383 => "T_STRING_CAST",
        384 => "T_ARRAY_CAST",
        385 => "T_OBJECT_CAST",
        386 => "T_BOOL_CAST",
        387 => "T_UNSET_CAST",
        388 => "T_VOID_CAST",
        389 => "T_OBJECT_OPERATOR",
        390 => "T_NULLSAFE_OBJECT_OPERATOR",
        391 => "T_DOUBLE_ARROW",
        392 => "T_COMMENT",
        393 => "T_DOC_COMMENT",
        394 => "T_OPEN_TAG",
        395 => "T_OPEN_TAG_WITH_ECHO",
        396 => "T_CLOSE_TAG",
        397 => "T_WHITESPACE",
        398 => "T_START_HEREDOC",
        399 => "T_END_HEREDOC",
        400 => "T_DOLLAR_OPEN_CURLY_BRACES",
        401 => "T_CURLY_OPEN",
        402 => "T_DOUBLE_COLON",
        403 => "T_NS_SEPARATOR",
        404 => "T_ELLIPSIS",
        405 => "T_COALESCE",
        406 => "T_POW",
        407 => "T_POW_EQUAL",
        408 => "T_PIPE",
        409 => "T_AMPERSAND_FOLLOWED_BY_VAR_OR_VARARG",
        410 => "T_AMPERSAND_NOT_FOLLOWED_BY_VAR_OR_VARARG",
        411 => "T_BAD_CHARACTER",
        _ => return None,
    };
    Some(name)
}

/// Token ids classified as "ignorable" by PhpToken::isIgnorable():
/// whitespace, comments, open/close tag, inline HTML.
pub fn token_id_is_ignorable(id: i64) -> bool {
    use crate::php_scanner as ps;
    [
        ps::T_INLINE_HTML,
        ps::T_COMMENT,
        ps::T_DOC_COMMENT,
        ps::T_OPEN_TAG,
        ps::T_OPEN_TAG_WITH_ECHO,
        ps::T_CLOSE_TAG,
        ps::T_WHITESPACE,
    ]
    .contains(&(id as i32))
}

/// Map identifier bytes to keyword token kind
pub fn keyword_or_identifier(ident: &[u8]) -> TokenKind {
    keyword_kind(ident).unwrap_or_else(|| TokenKind::Identifier(ident.to_vec()))
}

/// The keyword `ident` is (case-insensitively), if any.
fn keyword_kind(ident: &[u8]) -> Option<TokenKind> {
    // (keywords are 2 to 12 bytes long)
    if !(2..=12).contains(&ident.len()) {
        return None;
    }
    let mut buf = [0u8; 12];
    let lower = &mut buf[..ident.len()];
    for (d, s) in lower.iter_mut().zip(ident) {
        *d = s.to_ascii_lowercase();
    }
    Some(match &*lower {
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
        _ => return None,
    })
}
