//! Token kinds and token records.
//!
//! The numeric ids of the named tokens are the ones PHP 8.5 exposes through
//! the tokenizer extension (`T_*` constants; observed on PHP 8.5 with
//! `get_defined_constants(true)["tokenizer"]`). Single-character tokens use
//! their byte value as id, as `token_get_all()` returns them as plain strings.

macro_rules! tokens {
    (
        chars { $($cname:ident = $cval:literal,)* }
        named { $($name:ident = $id:literal => $pname:literal,)* }
    ) => {
        /// A token kind. Named kinds carry PHP's `T_*` id, character tokens
        /// their byte value.
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
        #[repr(u16)]
        pub enum T {
            /// End of input.
            Eof = 0,
            $($cname = $cval as u16,)*
            $($name = $id,)*
        }

        impl T {
            /// The PHP token id (`T_*` value or character code).
            #[inline]
            pub fn id(self) -> u16 {
                self as u16
            }

            /// The `T_*` constant name, as `token_name()` returns it, for
            /// named tokens; `None` for character tokens and EOF.
            pub fn php_name(self) -> Option<&'static str> {
                match self {
                    $(T::$name => Some($pname),)*
                    _ => None,
                }
            }

            /// The token kind for a PHP token id.
            pub fn from_id(id: u16) -> Option<T> {
                if id < 256 {
                    return match id as u8 {
                        0 => Some(T::Eof),
                        $($cval => Some(T::$cname),)*
                        _ => None,
                    };
                }
                match id {
                    $($id => Some(T::$name),)*
                    _ => None,
                }
            }

            /// All named tokens.
            pub const NAMED: &'static [T] = &[$(T::$name,)*];
        }
    };
}

tokens! {
    chars {
        Semicolon = b';',
        Colon = b':',
        Comma = b',',
        Dot = b'.',
        LBracket = b'[',
        RBracket = b']',
        LParen = b'(',
        RParen = b')',
        Pipe = b'|',
        Caret = b'^',
        Amp = b'&',
        Plus = b'+',
        Minus = b'-',
        Slash = b'/',
        Star = b'*',
        Eq = b'=',
        Percent = b'%',
        Bang = b'!',
        Tilde = b'~',
        Dollar = b'$',
        Lt = b'<',
        Gt = b'>',
        Question = b'?',
        At = b'@',
        LBrace = b'{',
        RBrace = b'}',
        DoubleQuote = b'"',
        Backtick = b'`',
    }
    named {
        LNumber = 260 => "T_LNUMBER",
        DNumber = 261 => "T_DNUMBER",
        String = 262 => "T_STRING",
        NameFullyQualified = 263 => "T_NAME_FULLY_QUALIFIED",
        NameRelative = 264 => "T_NAME_RELATIVE",
        NameQualified = 265 => "T_NAME_QUALIFIED",
        Variable = 266 => "T_VARIABLE",
        InlineHtml = 267 => "T_INLINE_HTML",
        EncapsedAndWhitespace = 268 => "T_ENCAPSED_AND_WHITESPACE",
        ConstantEncapsedString = 269 => "T_CONSTANT_ENCAPSED_STRING",
        StringVarname = 270 => "T_STRING_VARNAME",
        NumString = 271 => "T_NUM_STRING",
        Include = 272 => "T_INCLUDE",
        IncludeOnce = 273 => "T_INCLUDE_ONCE",
        Eval = 274 => "T_EVAL",
        Require = 275 => "T_REQUIRE",
        RequireOnce = 276 => "T_REQUIRE_ONCE",
        LogicalOr = 277 => "T_LOGICAL_OR",
        LogicalXor = 278 => "T_LOGICAL_XOR",
        LogicalAnd = 279 => "T_LOGICAL_AND",
        Print = 280 => "T_PRINT",
        Yield = 281 => "T_YIELD",
        YieldFrom = 282 => "T_YIELD_FROM",
        Instanceof = 283 => "T_INSTANCEOF",
        New = 284 => "T_NEW",
        Clone = 285 => "T_CLONE",
        Exit = 286 => "T_EXIT",
        If = 287 => "T_IF",
        Elseif = 288 => "T_ELSEIF",
        Else = 289 => "T_ELSE",
        Endif = 290 => "T_ENDIF",
        Echo = 291 => "T_ECHO",
        Do = 292 => "T_DO",
        While = 293 => "T_WHILE",
        Endwhile = 294 => "T_ENDWHILE",
        For = 295 => "T_FOR",
        Endfor = 296 => "T_ENDFOR",
        Foreach = 297 => "T_FOREACH",
        Endforeach = 298 => "T_ENDFOREACH",
        Declare = 299 => "T_DECLARE",
        Enddeclare = 300 => "T_ENDDECLARE",
        As = 301 => "T_AS",
        Switch = 302 => "T_SWITCH",
        Endswitch = 303 => "T_ENDSWITCH",
        Case = 304 => "T_CASE",
        Default = 305 => "T_DEFAULT",
        Match = 306 => "T_MATCH",
        Break = 307 => "T_BREAK",
        Continue = 308 => "T_CONTINUE",
        Goto = 309 => "T_GOTO",
        Function = 310 => "T_FUNCTION",
        Fn = 311 => "T_FN",
        Const = 312 => "T_CONST",
        Return = 313 => "T_RETURN",
        Try = 314 => "T_TRY",
        Catch = 315 => "T_CATCH",
        Finally = 316 => "T_FINALLY",
        Throw = 317 => "T_THROW",
        Use = 318 => "T_USE",
        Insteadof = 319 => "T_INSTEADOF",
        Global = 320 => "T_GLOBAL",
        Static = 321 => "T_STATIC",
        Abstract = 322 => "T_ABSTRACT",
        Final = 323 => "T_FINAL",
        Private = 324 => "T_PRIVATE",
        Protected = 325 => "T_PROTECTED",
        Public = 326 => "T_PUBLIC",
        PrivateSet = 327 => "T_PRIVATE_SET",
        ProtectedSet = 328 => "T_PROTECTED_SET",
        PublicSet = 329 => "T_PUBLIC_SET",
        Readonly = 330 => "T_READONLY",
        Var = 331 => "T_VAR",
        Unset = 332 => "T_UNSET",
        Isset = 333 => "T_ISSET",
        Empty = 334 => "T_EMPTY",
        HaltCompiler = 335 => "T_HALT_COMPILER",
        Class = 336 => "T_CLASS",
        Trait = 337 => "T_TRAIT",
        Interface = 338 => "T_INTERFACE",
        Enum = 339 => "T_ENUM",
        Extends = 340 => "T_EXTENDS",
        Implements = 341 => "T_IMPLEMENTS",
        Namespace = 342 => "T_NAMESPACE",
        List = 343 => "T_LIST",
        Array = 344 => "T_ARRAY",
        Callable = 345 => "T_CALLABLE",
        Line = 346 => "T_LINE",
        File = 347 => "T_FILE",
        Dir = 348 => "T_DIR",
        ClassC = 349 => "T_CLASS_C",
        TraitC = 350 => "T_TRAIT_C",
        MethodC = 351 => "T_METHOD_C",
        FuncC = 352 => "T_FUNC_C",
        PropertyC = 353 => "T_PROPERTY_C",
        NsC = 354 => "T_NS_C",
        Attribute = 355 => "T_ATTRIBUTE",
        PlusEqual = 356 => "T_PLUS_EQUAL",
        MinusEqual = 357 => "T_MINUS_EQUAL",
        MulEqual = 358 => "T_MUL_EQUAL",
        DivEqual = 359 => "T_DIV_EQUAL",
        ConcatEqual = 360 => "T_CONCAT_EQUAL",
        ModEqual = 361 => "T_MOD_EQUAL",
        AndEqual = 362 => "T_AND_EQUAL",
        OrEqual = 363 => "T_OR_EQUAL",
        XorEqual = 364 => "T_XOR_EQUAL",
        SlEqual = 365 => "T_SL_EQUAL",
        SrEqual = 366 => "T_SR_EQUAL",
        CoalesceEqual = 367 => "T_COALESCE_EQUAL",
        BooleanOr = 368 => "T_BOOLEAN_OR",
        BooleanAnd = 369 => "T_BOOLEAN_AND",
        IsEqual = 370 => "T_IS_EQUAL",
        IsNotEqual = 371 => "T_IS_NOT_EQUAL",
        IsIdentical = 372 => "T_IS_IDENTICAL",
        IsNotIdentical = 373 => "T_IS_NOT_IDENTICAL",
        IsSmallerOrEqual = 374 => "T_IS_SMALLER_OR_EQUAL",
        IsGreaterOrEqual = 375 => "T_IS_GREATER_OR_EQUAL",
        Spaceship = 376 => "T_SPACESHIP",
        Sl = 377 => "T_SL",
        Sr = 378 => "T_SR",
        Inc = 379 => "T_INC",
        Dec = 380 => "T_DEC",
        IntCast = 381 => "T_INT_CAST",
        DoubleCast = 382 => "T_DOUBLE_CAST",
        StringCast = 383 => "T_STRING_CAST",
        ArrayCast = 384 => "T_ARRAY_CAST",
        ObjectCast = 385 => "T_OBJECT_CAST",
        BoolCast = 386 => "T_BOOL_CAST",
        UnsetCast = 387 => "T_UNSET_CAST",
        VoidCast = 388 => "T_VOID_CAST",
        ObjectOperator = 389 => "T_OBJECT_OPERATOR",
        NullsafeObjectOperator = 390 => "T_NULLSAFE_OBJECT_OPERATOR",
        DoubleArrow = 391 => "T_DOUBLE_ARROW",
        Comment = 392 => "T_COMMENT",
        DocComment = 393 => "T_DOC_COMMENT",
        OpenTag = 394 => "T_OPEN_TAG",
        OpenTagWithEcho = 395 => "T_OPEN_TAG_WITH_ECHO",
        CloseTag = 396 => "T_CLOSE_TAG",
        Whitespace = 397 => "T_WHITESPACE",
        StartHeredoc = 398 => "T_START_HEREDOC",
        EndHeredoc = 399 => "T_END_HEREDOC",
        DollarOpenCurlyBraces = 400 => "T_DOLLAR_OPEN_CURLY_BRACES",
        CurlyOpen = 401 => "T_CURLY_OPEN",
        DoubleColon = 402 => "T_DOUBLE_COLON",
        NsSeparator = 403 => "T_NS_SEPARATOR",
        Ellipsis = 404 => "T_ELLIPSIS",
        Coalesce = 405 => "T_COALESCE",
        Pow = 406 => "T_POW",
        PowEqual = 407 => "T_POW_EQUAL",
        PipeGt = 408 => "T_PIPE",
        AmpFollowedByVarOrVararg = 409 => "T_AMPERSAND_FOLLOWED_BY_VAR_OR_VARARG",
        AmpNotFollowedByVarOrVararg = 410 => "T_AMPERSAND_NOT_FOLLOWED_BY_VAR_OR_VARARG",
        BadCharacter = 411 => "T_BAD_CHARACTER",
    }
}

impl T {
    /// `token_name()`: named tokens give their `T_*` name; anything else is
    /// "UNKNOWN" (observed on PHP 8.5: `token_name(59)` is "UNKNOWN").
    pub fn token_name(id: i64) -> &'static str {
        if !(0..=u16::MAX as i64).contains(&id) {
            return "UNKNOWN";
        }
        match T::from_id(id as u16).and_then(T::php_name) {
            Some(n) => n,
            None => "UNKNOWN",
        }
    }

    /// Whether the token is a single character (returned as a plain string
    /// by `token_get_all()`).
    #[inline]
    pub fn is_char(self) -> bool {
        (self as u16) < 256 && self != T::Eof
    }

    /// Whitespace and comments: skipped by the parser.
    #[inline]
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            T::Whitespace | T::Comment | T::DocComment | T::OpenTag
        )
    }

    /// Keywords that the grammar accepts as identifiers in member names
    /// (methods, class constants, properties after `->`/`::`), named
    /// arguments and similar positions ("semi-reserved" words, PHP manual:
    /// "List of Keywords"; context-sensitive lexer RFC).
    pub fn is_semi_reserved(self) -> bool {
        self.is_reserved_non_modifier()
            || matches!(
                self,
                T::Static
                    | T::Abstract
                    | T::Final
                    | T::Private
                    | T::Protected
                    | T::Public
                    | T::Readonly
            )
    }

    /// Keywords other than the member modifiers.
    pub fn is_reserved_non_modifier(self) -> bool {
        matches!(
            self,
            T::Include
                | T::IncludeOnce
                | T::Eval
                | T::Require
                | T::RequireOnce
                | T::LogicalOr
                | T::LogicalXor
                | T::LogicalAnd
                | T::Instanceof
                | T::New
                | T::Clone
                | T::Exit
                | T::If
                | T::Elseif
                | T::Else
                | T::Endif
                | T::Echo
                | T::Do
                | T::While
                | T::Endwhile
                | T::For
                | T::Endfor
                | T::Foreach
                | T::Endforeach
                | T::Declare
                | T::Enddeclare
                | T::As
                | T::Try
                | T::Catch
                | T::Finally
                | T::Throw
                | T::Use
                | T::Insteadof
                | T::Global
                | T::Var
                | T::Unset
                | T::Isset
                | T::Empty
                | T::Continue
                | T::Goto
                | T::Function
                | T::Const
                | T::Return
                | T::Print
                | T::Yield
                | T::List
                | T::Switch
                | T::Endswitch
                | T::Case
                | T::Default
                | T::Break
                | T::Array
                | T::Callable
                | T::Extends
                | T::Implements
                | T::Namespace
                | T::Trait
                | T::Interface
                | T::Class
                | T::ClassC
                | T::TraitC
                | T::FuncC
                | T::MethodC
                | T::Line
                | T::File
                | T::Dir
                | T::NsC
                | T::Fn
                | T::Match
                | T::Enum
                | T::PropertyC
        )
    }
}

/// A lexed token: kind, byte range in the source and the line it starts on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: T,
    /// Extra lexer information (see the `TF_*` flags).
    pub flags: u16,
    pub start: u32,
    pub end: u32,
    pub line: u32,
}

/// The token's lexer flagged an error (see [`crate::lexer::LexError`]); the
/// parser reports it when it reaches the token.
pub const TF_ERROR: u16 = 1;
/// A `T_CLOSE_TAG` that the parser sees as `;`, or a `T_OPEN_TAG_WITH_ECHO`
/// seen as `echo`.
pub const TF_TAG: u16 = 2;
/// A double-quoted `T_CONSTANT_ENCAPSED_STRING` or a heredoc start.
pub const TF_DOUBLE: u16 = 4;
/// A string literal that contains a backslash (needs escape processing).
pub const TF_ESCAPES: u16 = 8;
/// A `b` prefix on a string literal or heredoc.
pub const TF_BINARY_PREFIX: u16 = 16;

impl Token {
    #[inline]
    pub fn text<'a>(&self, src: &'a [u8]) -> &'a [u8] {
        &src[self.start as usize..self.end as usize]
    }
}
