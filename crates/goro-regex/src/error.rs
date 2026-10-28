//! Compile and match errors.
//!
//! The compile error texts are the ones PCRE2 reports and PHP prints as
//! "preg_match(): Compilation failed: <message> at offset N" (observed on
//! PHP 8.5 with PCRE2 10.46, and in the `--EXPECT--` sections of
//! ext/pcre/tests). pcre2api: "the textual error messages that are obtained by
//! calling pcre2_get_error_message() ... should be self-explanatory".

use std::fmt;

/// Why a pattern failed to compile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    EndBackslash,
    EndBackslashC,
    UnknownEscape,
    QuantifierOutOfOrder,
    QuantifierTooBig,
    MissingSquareBracket,
    EscapeInvalidInClass,
    ClassRangeOrder,
    QuantifierInvalid,
    InvalidAfterParensQuery,
    PosixClassNotInClass,
    PosixNoSupportCollating,
    MissingClosingParenthesis,
    BadSubpatternReference,
    MissingCommentClosing,
    ParenthesesNestTooDeep,
    PatternTooLarge,
    UnmatchedClosingParenthesis,
    MissingConditionClosing,
    LookbehindNotFixedLength,
    ZeroRelativeReference,
    TooManyConditionBranches,
    ConditionAtomicAssertionExpected,
    BadRelativeReference,
    UnknownPosixClass,
    CodePointTooBig,
    LookbehindBadBackslashC,
    UnsupportedEscapeSequence,
    CalloutNumberTooBig,
    MissingCalloutClosing,
    EscapeInvalidInVerb,
    UnrecognizedAfterQueryP,
    MissingNameTerminator,
    DuplicateSubpatternName,
    InvalidSubpatternName,
    MalformedUnicodeProperty,
    UnknownUnicodeProperty,
    SubpatternNameTooLong,
    TooManyNamedSubpatterns,
    ClassInvalidRange,
    OctalByteTooBig,
    DefineTooManyBranches,
    BackslashONoBrace,
    BackslashGSyntax,
    ParensQueryRMissingClosing,
    VerbUnknown,
    SubpatternNumberTooBig,
    SubpatternNameExpected,
    InvalidOctal,
    SubpatternNamesMismatch,
    MarkMissingArgument,
    InvalidHexadecimal,
    BackslashCSyntax,
    BackslashKSyntax,
    BackslashNInClass,
    CalloutStringTooLong,
    UnicodeDisallowedCodePoint,
    VerbNameTooLong,
    BackslashUCodePointTooBig,
    MissingOctalOrHexDigits,
    VersionConditionSyntax,
    CalloutNoStringDelimiter,
    CalloutBadStringDelimiter,
    QueryBarjxNestTooDeep,
    PatternTooComplicated,
    LookbehindTooLong,
    TooManyCaptures,
    ConditionAssertionExpected,
    BackslashNNoUtf,
    InvalidHyphenInOptions,
    AlphaAssertionUnknown,
    BackslashKInLookaround,
    VariableLengthLookbehindTooLong,
    ExtClassUnexpectedChar,
    ExtClassEmpty,
    ExtClassNoOperand,
    ExtClassNoOperator,
    ExtClassExpectedOperand,
    ExtClassMissingParen,
    /// Invalid UTF-8 in the pattern (PCRE2's negative UTF-8 error codes).
    Utf8(Utf8Error),
    /// A matching resource problem detected at compile time (unused).
    Internal,
}

/// The kinds of invalid UTF-8 that PCRE2 distinguishes (pcre2unicode:
/// "Errors in UTF-8 strings"). The numbers are PCRE2_ERROR_UTF8_ERRn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Utf8Error {
    /// ERR1..ERR5: the string ends with a truncated character, `n` bytes missing.
    Truncated(u8),
    /// ERR6..ERR10: byte `n` (2..=6) of a character does not have top bits 0b10.
    BadContinuation(u8),
    /// ERR11: a 5-byte character.
    FiveByte,
    /// ERR12: a 6-byte character.
    SixByte,
    /// ERR13: a 4-byte character above 0x10ffff.
    TooLarge,
    /// ERR14: a surrogate.
    Surrogate,
    /// ERR15..ERR19: an overlong `n`-byte sequence (2..=6).
    Overlong(u8),
    /// ERR20: an isolated continuation byte.
    IsolatedContinuation,
    /// ERR21: 0xfe or 0xff.
    IllegalByte,
}

impl Utf8Error {
    /// The PCRE2 error message (without the "UTF-8 error: " prefix).
    pub fn message(self) -> String {
        match self {
            Utf8Error::Truncated(1) => "1 byte missing at end".to_string(),
            Utf8Error::Truncated(n) => format!("{n} bytes missing at end"),
            Utf8Error::BadContinuation(n) => format!("byte {n} top bits not 0x80"),
            Utf8Error::FiveByte => "5-byte character is not allowed (RFC 3629)".to_string(),
            Utf8Error::SixByte => "6-byte character is not allowed (RFC 3629)".to_string(),
            Utf8Error::TooLarge => "code points greater than 0x10ffff are not defined".to_string(),
            Utf8Error::Surrogate => "code points 0xd800-0xdfff are not defined".to_string(),
            Utf8Error::Overlong(n) => format!("overlong {n}-byte sequence"),
            Utf8Error::IsolatedContinuation => "isolated byte with 0x80 bit set".to_string(),
            Utf8Error::IllegalByte => "illegal byte (0xfe or 0xff)".to_string(),
        }
    }

    /// The PCRE2 negative error code (PCRE2_ERROR_UTF8_ERR1 is -3).
    pub fn code(self) -> i32 {
        let n = match self {
            Utf8Error::Truncated(n) => n as i32,
            Utf8Error::BadContinuation(n) => 4 + n as i32,
            Utf8Error::FiveByte => 11,
            Utf8Error::SixByte => 12,
            Utf8Error::TooLarge => 13,
            Utf8Error::Surrogate => 14,
            Utf8Error::Overlong(n) => 13 + n as i32,
            Utf8Error::IsolatedContinuation => 20,
            Utf8Error::IllegalByte => 21,
        };
        -2 - n
    }
}

impl ErrorKind {
    /// The PCRE2 message for this error.
    pub fn message(self) -> String {
        use ErrorKind::*;
        let s = match self {
            EndBackslash => "\\ at end of pattern",
            EndBackslashC => "\\c at end of pattern",
            UnknownEscape => "unrecognized character follows \\",
            QuantifierOutOfOrder => "numbers out of order in {} quantifier",
            QuantifierTooBig => "number too big in {} quantifier",
            MissingSquareBracket => "missing terminating ] for character class",
            EscapeInvalidInClass => "escape sequence is invalid in character class",
            ClassRangeOrder => "range out of order in character class",
            QuantifierInvalid => "quantifier does not follow a repeatable item",
            InvalidAfterParensQuery => "unrecognized character after (? or (?-",
            PosixClassNotInClass => "POSIX named classes are supported only within a class",
            PosixNoSupportCollating => "POSIX collating elements are not supported",
            MissingClosingParenthesis => "missing closing parenthesis",
            BadSubpatternReference => "reference to non-existent subpattern",
            MissingCommentClosing => "missing ) after (?# comment",
            ParenthesesNestTooDeep => "parentheses are too deeply nested",
            PatternTooLarge => "regular expression is too large",
            UnmatchedClosingParenthesis => "unmatched closing parenthesis",
            MissingConditionClosing => "missing closing parenthesis for condition",
            LookbehindNotFixedLength => "length of lookbehind assertion is not limited",
            ZeroRelativeReference => "a relative value of zero is not allowed",
            TooManyConditionBranches => "conditional subpattern contains more than two branches",
            ConditionAtomicAssertionExpected => "atomic assertion expected after (?( or (?(?C)",
            BadRelativeReference => "digit expected after (?+ or (?-",
            UnknownPosixClass => "unknown POSIX class name",
            CodePointTooBig => "character code point value in \\x{} or \\o{} is too large",
            LookbehindBadBackslashC => "\\C is not allowed in a lookbehind assertion in UTF-8 mode",
            UnsupportedEscapeSequence => {
                "PCRE2 does not support \\F, \\L, \\l, \\N{name}, \\U, or \\u"
            }
            CalloutNumberTooBig => "number after (?C is greater than 255",
            MissingCalloutClosing => "closing parenthesis for (?C expected",
            EscapeInvalidInVerb => "invalid escape sequence in (*VERB) name",
            UnrecognizedAfterQueryP => "unrecognized character after (?P",
            MissingNameTerminator => "syntax error in subpattern name (missing terminator?)",
            DuplicateSubpatternName => {
                "two named subpatterns have the same name (PCRE2_DUPNAMES not set)"
            }
            InvalidSubpatternName => "subpattern name must start with a non-digit",
            MalformedUnicodeProperty => "malformed \\P or \\p sequence",
            UnknownUnicodeProperty => "unknown property after \\P or \\p",
            SubpatternNameTooLong => "subpattern name is too long (maximum 128 code units)",
            TooManyNamedSubpatterns => "too many named subpatterns (maximum 10000)",
            ClassInvalidRange => "invalid range in character class",
            OctalByteTooBig => "octal value is greater than \\377 in 8-bit non-UTF-8 mode",
            DefineTooManyBranches => "DEFINE subpattern contains more than one branch",
            BackslashONoBrace => "missing opening brace after \\o",
            BackslashGSyntax => {
                "\\g is not followed by a braced, angle-bracketed, or quoted name/number or by a plain number"
            }
            ParensQueryRMissingClosing => {
                "(?R (recursive pattern call) must be followed by a closing parenthesis"
            }
            VerbUnknown => "(*VERB) not recognized or malformed",
            SubpatternNumberTooBig => "subpattern number is too big",
            SubpatternNameExpected => "subpattern name expected",
            InvalidOctal => "non-octal character in \\o{} (closing brace missing?)",
            SubpatternNamesMismatch => {
                "different names for subpatterns of the same number are not allowed"
            }
            MarkMissingArgument => "(*MARK) must have an argument",
            InvalidHexadecimal => "non-hex character in \\x{} (closing brace missing?)",
            BackslashCSyntax => "\\c must be followed by a printable ASCII character",
            BackslashKSyntax => "\\k is not followed by a braced, angle-bracketed, or quoted name",
            BackslashNInClass => "\\N is not supported in a class",
            CalloutStringTooLong => "callout string is too long",
            UnicodeDisallowedCodePoint => "disallowed Unicode code point (>= 0xd800 && <= 0xdfff)",
            VerbNameTooLong => "name is too long in (*MARK), (*PRUNE), (*SKIP), or (*THEN)",
            BackslashUCodePointTooBig => {
                "character code point value in \\u.... sequence is too large"
            }
            MissingOctalOrHexDigits => "digits missing after \\x or in \\x{} or \\o{} or \\N{U+}",
            VersionConditionSyntax => "syntax error or number too big in (?(VERSION condition",
            CalloutNoStringDelimiter => {
                "missing terminating delimiter for callout with string argument"
            }
            CalloutBadStringDelimiter => "unrecognized string delimiter follows (?C",
            QueryBarjxNestTooDeep => "(?| and/or (?J: or (?x: parentheses are too deeply nested",
            PatternTooComplicated => "regular expression is too complicated",
            LookbehindTooLong => "lookbehind assertion is too long",
            TooManyCaptures => "too many capturing groups (maximum 65535)",
            ConditionAssertionExpected => "assertion expected after (?( or (?(?C)",
            BackslashNNoUtf => "\\N{U+dddd} is supported only in Unicode (UTF) mode",
            InvalidHyphenInOptions => "invalid hyphen in option setting",
            AlphaAssertionUnknown => "(*alpha_assertion) not recognized",
            BackslashKInLookaround => {
                "\\K is not allowed in lookarounds (but see PCRE2_EXTRA_ALLOW_LOOKAROUND_BSK)"
            }
            VariableLengthLookbehindTooLong => {
                "branch too long in variable-length lookbehind assertion"
            }
            ExtClassUnexpectedChar => "unexpected character in (?[...]) extended character class",
            ExtClassEmpty => "empty expression in extended character class",
            ExtClassNoOperand => {
                "unexpected operator in extended character class (no preceding operand)"
            }
            ExtClassNoOperator => {
                "unexpected expression in extended character class (no preceding operator)"
            }
            ExtClassExpectedOperand => {
                "expected operand after operator in extended character class"
            }
            ExtClassMissingParen => {
                "terminating ] with no following closing parenthesis in (?[...]"
            }
            Utf8(e) => return format!("UTF-8 error: {}", e.message()),
            Internal => "internal error: unexpected repeat",
        };
        s.to_string()
    }
}

/// A compile error: what went wrong and where (a byte offset into the
/// pattern, as PCRE2 reports it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileError {
    pub kind: ErrorKind,
    pub offset: usize,
}

impl CompileError {
    pub fn new(kind: ErrorKind, offset: usize) -> CompileError {
        CompileError { kind, offset }
    }

    /// The message as PCRE2 formats it, without the offset.
    pub fn message(&self) -> String {
        self.kind.message()
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at offset {}", self.kind.message(), self.offset)
    }
}

impl std::error::Error for CompileError {}

/// Why a match attempt stopped without a result. These map to PHP's
/// PREG_*_ERROR constants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchError {
    /// The match limit (pcre.backtrack_limit) was reached:
    /// PREG_BACKTRACK_LIMIT_ERROR.
    MatchLimit,
    /// The depth limit (pcre.recursion_limit) was reached:
    /// PREG_RECURSION_LIMIT_ERROR.
    DepthLimit,
    /// The emulated JIT stack ran out: PREG_JIT_STACKLIMIT_ERROR.
    JitStackLimit,
    /// The heap limit was reached (PREG_INTERNAL_ERROR in PHP).
    HeapLimit,
    /// The subject is not valid UTF-8 at `offset`: PREG_BAD_UTF8_ERROR.
    BadUtf8 { error: Utf8Error, offset: usize },
    /// The start offset is inside a UTF-8 character:
    /// PREG_BAD_UTF8_OFFSET_ERROR.
    BadUtf8Offset,
    /// A group was called recursively at the same position twice
    /// (pcre2api: PCRE2_ERROR_RECURSELOOP); PREG_INTERNAL_ERROR in PHP.
    RecurseLoop,
    /// The start offset is past the end of the subject.
    BadOffset,
}

impl fmt::Display for MatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MatchError::MatchLimit => f.write_str("match limit exceeded"),
            MatchError::DepthLimit => f.write_str("matching depth limit exceeded"),
            MatchError::JitStackLimit => f.write_str("JIT stack limit reached"),
            MatchError::HeapLimit => f.write_str("heap limit exceeded"),
            MatchError::BadUtf8 { error, offset } => {
                write!(f, "UTF-8 error: {} at offset {}", error.message(), offset)
            }
            MatchError::BadUtf8Offset => f.write_str("bad offset into UTF string"),
            MatchError::RecurseLoop => f.write_str(
                "recursion loop detected: a group was called recursively at the same position",
            ),
            MatchError::BadOffset => f.write_str("bad offset value"),
        }
    }
}

impl std::error::Error for MatchError {}
