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
            TokenKind::Eof => "end of file".into(),
            _ => format!("{:?}", self),
        }
    }
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
