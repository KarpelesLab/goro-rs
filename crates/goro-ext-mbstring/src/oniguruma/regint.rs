//! Shared definitions (port of oniguruma.h / regint.h / regparse.h).

#![allow(dead_code)]

use super::enc::Encoding;

/* ===== options ===== */

pub const ONIG_OPTION_NONE: u32 = 0;
pub const ONIG_OPTION_IGNORECASE: u32 = 1;
pub const ONIG_OPTION_EXTEND: u32 = 1 << 1;
pub const ONIG_OPTION_MULTILINE: u32 = 1 << 2;
pub const ONIG_OPTION_SINGLELINE: u32 = 1 << 3;
pub const ONIG_OPTION_FIND_LONGEST: u32 = 1 << 4;
pub const ONIG_OPTION_FIND_NOT_EMPTY: u32 = 1 << 5;
pub const ONIG_OPTION_NEGATE_SINGLELINE: u32 = 1 << 6;
pub const ONIG_OPTION_DONT_CAPTURE_GROUP: u32 = 1 << 7;
pub const ONIG_OPTION_CAPTURE_GROUP: u32 = 1 << 8;
pub const ONIG_OPTION_NOTBOL: u32 = 1 << 9;
pub const ONIG_OPTION_NOTEOL: u32 = 1 << 10;
pub const ONIG_OPTION_POSIX_REGION: u32 = 1 << 11;
pub const ONIG_OPTION_CHECK_VALIDITY_OF_STRING: u32 = 1 << 12;
pub const ONIG_OPTION_IGNORECASE_IS_ASCII: u32 = 1 << 15;
pub const ONIG_OPTION_WORD_IS_ASCII: u32 = 1 << 16;
pub const ONIG_OPTION_DIGIT_IS_ASCII: u32 = 1 << 17;
pub const ONIG_OPTION_SPACE_IS_ASCII: u32 = 1 << 18;
pub const ONIG_OPTION_POSIX_IS_ASCII: u32 = 1 << 19;
pub const ONIG_OPTION_TEXT_SEGMENT_EXTENDED_GRAPHEME_CLUSTER: u32 = 1 << 20;
pub const ONIG_OPTION_TEXT_SEGMENT_WORD: u32 = 1 << 21;
pub const ONIG_OPTION_NOT_BEGIN_STRING: u32 = 1 << 22;
pub const ONIG_OPTION_NOT_END_STRING: u32 = 1 << 23;
pub const ONIG_OPTION_NOT_BEGIN_POSITION: u32 = 1 << 24;
pub const ONIG_OPTION_CALLBACK_EACH_MATCH: u32 = 1 << 25;
pub const ONIG_OPTION_MATCH_WHOLE_STRING: u32 = 1 << 26;

#[inline]
pub fn opton_singleline(o: u32) -> bool {
    o & ONIG_OPTION_SINGLELINE != 0
}
#[inline]
pub fn opton_multiline(o: u32) -> bool {
    o & ONIG_OPTION_MULTILINE != 0
}
#[inline]
pub fn opton_ignorecase(o: u32) -> bool {
    o & ONIG_OPTION_IGNORECASE != 0
}
#[inline]
pub fn opton_extend(o: u32) -> bool {
    o & ONIG_OPTION_EXTEND != 0
}
#[inline]
pub fn opton_word_ascii(o: u32) -> bool {
    o & (ONIG_OPTION_WORD_IS_ASCII | ONIG_OPTION_POSIX_IS_ASCII) != 0
}
#[inline]
pub fn opton_digit_ascii(o: u32) -> bool {
    o & (ONIG_OPTION_DIGIT_IS_ASCII | ONIG_OPTION_POSIX_IS_ASCII) != 0
}
#[inline]
pub fn opton_space_ascii(o: u32) -> bool {
    o & (ONIG_OPTION_SPACE_IS_ASCII | ONIG_OPTION_POSIX_IS_ASCII) != 0
}
#[inline]
pub fn opton_posix_ascii(o: u32) -> bool {
    o & ONIG_OPTION_POSIX_IS_ASCII != 0
}
#[inline]
pub fn opton_text_segment_word(o: u32) -> bool {
    o & ONIG_OPTION_TEXT_SEGMENT_WORD != 0
}
#[inline]
pub fn opton_find_longest(o: u32) -> bool {
    o & ONIG_OPTION_FIND_LONGEST != 0
}
#[inline]
pub fn opton_find_not_empty(o: u32) -> bool {
    o & ONIG_OPTION_FIND_NOT_EMPTY != 0
}
#[inline]
pub fn opton_dont_capture_group(o: u32) -> bool {
    o & ONIG_OPTION_DONT_CAPTURE_GROUP != 0
}
#[inline]
pub fn opton_capture_group(o: u32) -> bool {
    o & ONIG_OPTION_CAPTURE_GROUP != 0
}

/* ===== syntax ===== */

pub const ONIG_SYN_OP_VARIABLE_META_CHARACTERS: u32 = 1 << 0;
pub const ONIG_SYN_OP_DOT_ANYCHAR: u32 = 1 << 1;
pub const ONIG_SYN_OP_ASTERISK_ZERO_INF: u32 = 1 << 2;
pub const ONIG_SYN_OP_ESC_ASTERISK_ZERO_INF: u32 = 1 << 3;
pub const ONIG_SYN_OP_PLUS_ONE_INF: u32 = 1 << 4;
pub const ONIG_SYN_OP_ESC_PLUS_ONE_INF: u32 = 1 << 5;
pub const ONIG_SYN_OP_QMARK_ZERO_ONE: u32 = 1 << 6;
pub const ONIG_SYN_OP_ESC_QMARK_ZERO_ONE: u32 = 1 << 7;
pub const ONIG_SYN_OP_BRACE_INTERVAL: u32 = 1 << 8;
pub const ONIG_SYN_OP_ESC_BRACE_INTERVAL: u32 = 1 << 9;
pub const ONIG_SYN_OP_VBAR_ALT: u32 = 1 << 10;
pub const ONIG_SYN_OP_ESC_VBAR_ALT: u32 = 1 << 11;
pub const ONIG_SYN_OP_LPAREN_SUBEXP: u32 = 1 << 12;
pub const ONIG_SYN_OP_ESC_LPAREN_SUBEXP: u32 = 1 << 13;
pub const ONIG_SYN_OP_ESC_AZ_BUF_ANCHOR: u32 = 1 << 14;
pub const ONIG_SYN_OP_ESC_CAPITAL_G_BEGIN_ANCHOR: u32 = 1 << 15;
pub const ONIG_SYN_OP_DECIMAL_BACKREF: u32 = 1 << 16;
pub const ONIG_SYN_OP_BRACKET_CC: u32 = 1 << 17;
pub const ONIG_SYN_OP_ESC_W_WORD: u32 = 1 << 18;
pub const ONIG_SYN_OP_ESC_LTGT_WORD_BEGIN_END: u32 = 1 << 19;
pub const ONIG_SYN_OP_ESC_B_WORD_BOUND: u32 = 1 << 20;
pub const ONIG_SYN_OP_ESC_S_WHITE_SPACE: u32 = 1 << 21;
pub const ONIG_SYN_OP_ESC_D_DIGIT: u32 = 1 << 22;
pub const ONIG_SYN_OP_LINE_ANCHOR: u32 = 1 << 23;
pub const ONIG_SYN_OP_POSIX_BRACKET: u32 = 1 << 24;
pub const ONIG_SYN_OP_QMARK_NON_GREEDY: u32 = 1 << 25;
pub const ONIG_SYN_OP_ESC_CONTROL_CHARS: u32 = 1 << 26;
pub const ONIG_SYN_OP_ESC_C_CONTROL: u32 = 1 << 27;
pub const ONIG_SYN_OP_ESC_OCTAL3: u32 = 1 << 28;
pub const ONIG_SYN_OP_ESC_X_HEX2: u32 = 1 << 29;
pub const ONIG_SYN_OP_ESC_X_BRACE_HEX8: u32 = 1 << 30;
pub const ONIG_SYN_OP_ESC_O_BRACE_OCTAL: u32 = 1 << 31;

pub const ONIG_SYN_OP2_ESC_CAPITAL_Q_QUOTE: u32 = 1 << 0;
pub const ONIG_SYN_OP2_QMARK_GROUP_EFFECT: u32 = 1 << 1;
pub const ONIG_SYN_OP2_OPTION_PERL: u32 = 1 << 2;
pub const ONIG_SYN_OP2_OPTION_RUBY: u32 = 1 << 3;
pub const ONIG_SYN_OP2_PLUS_POSSESSIVE_REPEAT: u32 = 1 << 4;
pub const ONIG_SYN_OP2_PLUS_POSSESSIVE_INTERVAL: u32 = 1 << 5;
pub const ONIG_SYN_OP2_CCLASS_SET_OP: u32 = 1 << 6;
pub const ONIG_SYN_OP2_QMARK_LT_NAMED_GROUP: u32 = 1 << 7;
pub const ONIG_SYN_OP2_ESC_K_NAMED_BACKREF: u32 = 1 << 8;
pub const ONIG_SYN_OP2_ESC_G_SUBEXP_CALL: u32 = 1 << 9;
pub const ONIG_SYN_OP2_ATMARK_CAPTURE_HISTORY: u32 = 1 << 10;
pub const ONIG_SYN_OP2_ESC_CAPITAL_C_BAR_CONTROL: u32 = 1 << 11;
pub const ONIG_SYN_OP2_ESC_CAPITAL_M_BAR_META: u32 = 1 << 12;
pub const ONIG_SYN_OP2_ESC_V_VTAB: u32 = 1 << 13;
pub const ONIG_SYN_OP2_ESC_U_HEX4: u32 = 1 << 14;
pub const ONIG_SYN_OP2_ESC_GNU_BUF_ANCHOR: u32 = 1 << 15;
pub const ONIG_SYN_OP2_ESC_P_BRACE_CHAR_PROPERTY: u32 = 1 << 16;
pub const ONIG_SYN_OP2_ESC_P_BRACE_CIRCUMFLEX_NOT: u32 = 1 << 17;
pub const ONIG_SYN_OP2_ESC_H_XDIGIT: u32 = 1 << 19;
pub const ONIG_SYN_OP2_INEFFECTIVE_ESCAPE: u32 = 1 << 20;
pub const ONIG_SYN_OP2_QMARK_LPAREN_IF_ELSE: u32 = 1 << 21;
pub const ONIG_SYN_OP2_ESC_CAPITAL_K_KEEP: u32 = 1 << 22;
pub const ONIG_SYN_OP2_ESC_CAPITAL_R_GENERAL_NEWLINE: u32 = 1 << 23;
pub const ONIG_SYN_OP2_ESC_CAPITAL_N_O_SUPER_DOT: u32 = 1 << 24;
pub const ONIG_SYN_OP2_QMARK_TILDE_ABSENT_GROUP: u32 = 1 << 25;
pub const ONIG_SYN_OP2_ESC_X_Y_TEXT_SEGMENT: u32 = 1 << 26;
pub const ONIG_SYN_OP2_QMARK_PERL_SUBEXP_CALL: u32 = 1 << 27;
pub const ONIG_SYN_OP2_QMARK_BRACE_CALLOUT_CONTENTS: u32 = 1 << 28;
pub const ONIG_SYN_OP2_ASTERISK_CALLOUT_NAME: u32 = 1 << 29;
pub const ONIG_SYN_OP2_OPTION_ONIGURUMA: u32 = 1 << 30;
pub const ONIG_SYN_OP2_QMARK_CAPITAL_P_NAME: u32 = 1 << 31;

pub const ONIG_SYN_CONTEXT_INDEP_ANCHORS: u32 = 1 << 31;
pub const ONIG_SYN_CONTEXT_INDEP_REPEAT_OPS: u32 = 1 << 0;
pub const ONIG_SYN_CONTEXT_INVALID_REPEAT_OPS: u32 = 1 << 1;
pub const ONIG_SYN_ALLOW_UNMATCHED_CLOSE_SUBEXP: u32 = 1 << 2;
pub const ONIG_SYN_ALLOW_INVALID_INTERVAL: u32 = 1 << 3;
pub const ONIG_SYN_ALLOW_INTERVAL_LOW_ABBREV: u32 = 1 << 4;
pub const ONIG_SYN_STRICT_CHECK_BACKREF: u32 = 1 << 5;
pub const ONIG_SYN_DIFFERENT_LEN_ALT_LOOK_BEHIND: u32 = 1 << 6;
pub const ONIG_SYN_CAPTURE_ONLY_NAMED_GROUP: u32 = 1 << 7;
pub const ONIG_SYN_ALLOW_MULTIPLEX_DEFINITION_NAME: u32 = 1 << 8;
pub const ONIG_SYN_FIXED_INTERVAL_IS_GREEDY_ONLY: u32 = 1 << 9;
pub const ONIG_SYN_ISOLATED_OPTION_CONTINUE_BRANCH: u32 = 1 << 10;
pub const ONIG_SYN_VARIABLE_LEN_LOOK_BEHIND: u32 = 1 << 11;
pub const ONIG_SYN_PYTHON: u32 = 1 << 12;
pub const ONIG_SYN_WHOLE_OPTIONS: u32 = 1 << 13;
pub const ONIG_SYN_BRE_ANCHOR_AT_EDGE_OF_SUBEXP: u32 = 1 << 14;
pub const ONIG_SYN_NOT_NEWLINE_IN_NEGATIVE_CC: u32 = 1 << 20;
pub const ONIG_SYN_BACKSLASH_ESCAPE_IN_CC: u32 = 1 << 21;
pub const ONIG_SYN_ALLOW_EMPTY_RANGE_IN_CC: u32 = 1 << 22;
pub const ONIG_SYN_ALLOW_DOUBLE_RANGE_OP_IN_CC: u32 = 1 << 23;
pub const ONIG_SYN_WARN_CC_OP_NOT_ESCAPED: u32 = 1 << 24;
pub const ONIG_SYN_WARN_REDUNDANT_NESTED_REPEAT: u32 = 1 << 25;
pub const ONIG_SYN_ALLOW_INVALID_CODE_END_OF_RANGE_IN_CC: u32 = 1 << 26;
pub const ONIG_SYN_ALLOW_CHAR_TYPE_FOLLOWED_BY_MINUS_IN_CC: u32 = 1 << 27;

/// OnigSyntaxType (meta chars are fixed: escape is '\\', the variable
/// meta characters are never enabled by the syntaxes PHP uses)
#[derive(Debug, PartialEq, Eq)]
pub struct SyntaxType {
    pub op: u32,
    pub op2: u32,
    pub behavior: u32,
    pub options: u32,
}

impl SyntaxType {
    #[inline]
    pub fn op(&self, f: u32) -> bool {
        self.op & f != 0
    }
    #[inline]
    pub fn op2(&self, f: u32) -> bool {
        self.op2 & f != 0
    }
    #[inline]
    pub fn bv(&self, f: u32) -> bool {
        self.behavior & f != 0
    }
}

pub const MC_ESC: u32 = b'\\' as u32;

const SYN_POSIX_COMMON_OP: u32 = ONIG_SYN_OP_DOT_ANYCHAR
    | ONIG_SYN_OP_POSIX_BRACKET
    | ONIG_SYN_OP_DECIMAL_BACKREF
    | ONIG_SYN_OP_BRACKET_CC
    | ONIG_SYN_OP_ASTERISK_ZERO_INF
    | ONIG_SYN_OP_LINE_ANCHOR
    | ONIG_SYN_OP_ESC_CONTROL_CHARS;

const SYN_GNU_REGEX_OP: u32 = ONIG_SYN_OP_DOT_ANYCHAR
    | ONIG_SYN_OP_BRACKET_CC
    | ONIG_SYN_OP_POSIX_BRACKET
    | ONIG_SYN_OP_DECIMAL_BACKREF
    | ONIG_SYN_OP_BRACE_INTERVAL
    | ONIG_SYN_OP_LPAREN_SUBEXP
    | ONIG_SYN_OP_VBAR_ALT
    | ONIG_SYN_OP_ASTERISK_ZERO_INF
    | ONIG_SYN_OP_PLUS_ONE_INF
    | ONIG_SYN_OP_QMARK_ZERO_ONE
    | ONIG_SYN_OP_ESC_AZ_BUF_ANCHOR
    | ONIG_SYN_OP_ESC_CAPITAL_G_BEGIN_ANCHOR
    | ONIG_SYN_OP_ESC_W_WORD
    | ONIG_SYN_OP_ESC_B_WORD_BOUND
    | ONIG_SYN_OP_ESC_LTGT_WORD_BEGIN_END
    | ONIG_SYN_OP_ESC_S_WHITE_SPACE
    | ONIG_SYN_OP_ESC_D_DIGIT
    | ONIG_SYN_OP_LINE_ANCHOR;

const SYN_GNU_REGEX_BV: u32 = ONIG_SYN_CONTEXT_INDEP_ANCHORS
    | ONIG_SYN_CONTEXT_INDEP_REPEAT_OPS
    | ONIG_SYN_CONTEXT_INVALID_REPEAT_OPS
    | ONIG_SYN_ALLOW_INVALID_INTERVAL
    | ONIG_SYN_BACKSLASH_ESCAPE_IN_CC
    | ONIG_SYN_ALLOW_DOUBLE_RANGE_OP_IN_CC;

pub static ONIG_SYNTAX_POSIX_BASIC: SyntaxType = SyntaxType {
    op: SYN_POSIX_COMMON_OP | ONIG_SYN_OP_ESC_LPAREN_SUBEXP | ONIG_SYN_OP_ESC_BRACE_INTERVAL,
    op2: 0,
    behavior: ONIG_SYN_BRE_ANCHOR_AT_EDGE_OF_SUBEXP,
    options: ONIG_OPTION_SINGLELINE | ONIG_OPTION_MULTILINE,
};

pub static ONIG_SYNTAX_POSIX_EXTENDED: SyntaxType = SyntaxType {
    op: SYN_POSIX_COMMON_OP
        | ONIG_SYN_OP_LPAREN_SUBEXP
        | ONIG_SYN_OP_BRACE_INTERVAL
        | ONIG_SYN_OP_PLUS_ONE_INF
        | ONIG_SYN_OP_QMARK_ZERO_ONE
        | ONIG_SYN_OP_VBAR_ALT,
    op2: 0,
    behavior: ONIG_SYN_CONTEXT_INDEP_ANCHORS
        | ONIG_SYN_CONTEXT_INDEP_REPEAT_OPS
        | ONIG_SYN_CONTEXT_INVALID_REPEAT_OPS
        | ONIG_SYN_ALLOW_UNMATCHED_CLOSE_SUBEXP
        | ONIG_SYN_ALLOW_DOUBLE_RANGE_OP_IN_CC,
    options: ONIG_OPTION_SINGLELINE | ONIG_OPTION_MULTILINE,
};

pub static ONIG_SYNTAX_EMACS: SyntaxType = SyntaxType {
    op: ONIG_SYN_OP_DOT_ANYCHAR
        | ONIG_SYN_OP_BRACKET_CC
        | ONIG_SYN_OP_ESC_BRACE_INTERVAL
        | ONIG_SYN_OP_ESC_LPAREN_SUBEXP
        | ONIG_SYN_OP_ESC_VBAR_ALT
        | ONIG_SYN_OP_ASTERISK_ZERO_INF
        | ONIG_SYN_OP_PLUS_ONE_INF
        | ONIG_SYN_OP_QMARK_ZERO_ONE
        | ONIG_SYN_OP_DECIMAL_BACKREF
        | ONIG_SYN_OP_LINE_ANCHOR
        | ONIG_SYN_OP_ESC_CONTROL_CHARS,
    op2: ONIG_SYN_OP2_ESC_GNU_BUF_ANCHOR | ONIG_SYN_OP2_QMARK_GROUP_EFFECT,
    behavior: ONIG_SYN_ALLOW_EMPTY_RANGE_IN_CC,
    options: ONIG_OPTION_NONE,
};

pub static ONIG_SYNTAX_GREP: SyntaxType = SyntaxType {
    op: ONIG_SYN_OP_DOT_ANYCHAR
        | ONIG_SYN_OP_BRACKET_CC
        | ONIG_SYN_OP_POSIX_BRACKET
        | ONIG_SYN_OP_ESC_BRACE_INTERVAL
        | ONIG_SYN_OP_ESC_LPAREN_SUBEXP
        | ONIG_SYN_OP_ESC_VBAR_ALT
        | ONIG_SYN_OP_ASTERISK_ZERO_INF
        | ONIG_SYN_OP_ESC_PLUS_ONE_INF
        | ONIG_SYN_OP_ESC_QMARK_ZERO_ONE
        | ONIG_SYN_OP_LINE_ANCHOR
        | ONIG_SYN_OP_ESC_W_WORD
        | ONIG_SYN_OP_ESC_B_WORD_BOUND
        | ONIG_SYN_OP_ESC_LTGT_WORD_BEGIN_END
        | ONIG_SYN_OP_DECIMAL_BACKREF,
    op2: 0,
    behavior: ONIG_SYN_ALLOW_EMPTY_RANGE_IN_CC
        | ONIG_SYN_NOT_NEWLINE_IN_NEGATIVE_CC
        | ONIG_SYN_BRE_ANCHOR_AT_EDGE_OF_SUBEXP,
    options: ONIG_OPTION_NONE,
};

pub static ONIG_SYNTAX_GNU_REGEX: SyntaxType = SyntaxType {
    op: SYN_GNU_REGEX_OP,
    op2: 0,
    behavior: SYN_GNU_REGEX_BV,
    options: ONIG_OPTION_NONE,
};

pub static ONIG_SYNTAX_JAVA: SyntaxType = SyntaxType {
    op: (SYN_GNU_REGEX_OP
        | ONIG_SYN_OP_QMARK_NON_GREEDY
        | ONIG_SYN_OP_ESC_CONTROL_CHARS
        | ONIG_SYN_OP_ESC_C_CONTROL
        | ONIG_SYN_OP_ESC_OCTAL3
        | ONIG_SYN_OP_ESC_X_HEX2)
        & !(ONIG_SYN_OP_ESC_LTGT_WORD_BEGIN_END | ONIG_SYN_OP_POSIX_BRACKET),
    op2: ONIG_SYN_OP2_ESC_CAPITAL_Q_QUOTE
        | ONIG_SYN_OP2_QMARK_GROUP_EFFECT
        | ONIG_SYN_OP2_OPTION_PERL
        | ONIG_SYN_OP2_PLUS_POSSESSIVE_REPEAT
        | ONIG_SYN_OP2_PLUS_POSSESSIVE_INTERVAL
        | ONIG_SYN_OP2_CCLASS_SET_OP
        | ONIG_SYN_OP2_ESC_V_VTAB
        | ONIG_SYN_OP2_ESC_U_HEX4
        | ONIG_SYN_OP2_ESC_P_BRACE_CHAR_PROPERTY,
    behavior: SYN_GNU_REGEX_BV
        | ONIG_SYN_ISOLATED_OPTION_CONTINUE_BRANCH
        | ONIG_SYN_DIFFERENT_LEN_ALT_LOOK_BEHIND
        | ONIG_SYN_VARIABLE_LEN_LOOK_BEHIND
        | ONIG_SYN_ALLOW_CHAR_TYPE_FOLLOWED_BY_MINUS_IN_CC,
    options: ONIG_OPTION_SINGLELINE,
};

pub static ONIG_SYNTAX_PERL: SyntaxType = SyntaxType {
    op: (SYN_GNU_REGEX_OP
        | ONIG_SYN_OP_QMARK_NON_GREEDY
        | ONIG_SYN_OP_ESC_OCTAL3
        | ONIG_SYN_OP_ESC_X_HEX2
        | ONIG_SYN_OP_ESC_X_BRACE_HEX8
        | ONIG_SYN_OP_ESC_O_BRACE_OCTAL
        | ONIG_SYN_OP_ESC_CONTROL_CHARS
        | ONIG_SYN_OP_ESC_C_CONTROL)
        & !ONIG_SYN_OP_ESC_LTGT_WORD_BEGIN_END,
    op2: ONIG_SYN_OP2_ESC_CAPITAL_Q_QUOTE
        | ONIG_SYN_OP2_QMARK_GROUP_EFFECT
        | ONIG_SYN_OP2_OPTION_PERL
        | ONIG_SYN_OP2_PLUS_POSSESSIVE_REPEAT
        | ONIG_SYN_OP2_PLUS_POSSESSIVE_INTERVAL
        | ONIG_SYN_OP2_QMARK_LPAREN_IF_ELSE
        | ONIG_SYN_OP2_QMARK_TILDE_ABSENT_GROUP
        | ONIG_SYN_OP2_QMARK_BRACE_CALLOUT_CONTENTS
        | ONIG_SYN_OP2_ASTERISK_CALLOUT_NAME
        | ONIG_SYN_OP2_ESC_X_Y_TEXT_SEGMENT
        | ONIG_SYN_OP2_ESC_P_BRACE_CHAR_PROPERTY
        | ONIG_SYN_OP2_ESC_P_BRACE_CIRCUMFLEX_NOT
        | ONIG_SYN_OP2_ESC_CAPITAL_K_KEEP
        | ONIG_SYN_OP2_ESC_CAPITAL_R_GENERAL_NEWLINE
        | ONIG_SYN_OP2_ESC_CAPITAL_N_O_SUPER_DOT,
    behavior: SYN_GNU_REGEX_BV
        | ONIG_SYN_ISOLATED_OPTION_CONTINUE_BRANCH
        | ONIG_SYN_ALLOW_CHAR_TYPE_FOLLOWED_BY_MINUS_IN_CC,
    options: ONIG_OPTION_SINGLELINE,
};

pub static ONIG_SYNTAX_RUBY: SyntaxType = SyntaxType {
    op: (SYN_GNU_REGEX_OP
        | ONIG_SYN_OP_QMARK_NON_GREEDY
        | ONIG_SYN_OP_ESC_OCTAL3
        | ONIG_SYN_OP_ESC_X_HEX2
        | ONIG_SYN_OP_ESC_X_BRACE_HEX8
        | ONIG_SYN_OP_ESC_O_BRACE_OCTAL
        | ONIG_SYN_OP_ESC_CONTROL_CHARS
        | ONIG_SYN_OP_ESC_C_CONTROL)
        & !ONIG_SYN_OP_ESC_LTGT_WORD_BEGIN_END,
    op2: ONIG_SYN_OP2_QMARK_GROUP_EFFECT
        | ONIG_SYN_OP2_OPTION_RUBY
        | ONIG_SYN_OP2_QMARK_LT_NAMED_GROUP
        | ONIG_SYN_OP2_ESC_K_NAMED_BACKREF
        | ONIG_SYN_OP2_QMARK_LPAREN_IF_ELSE
        | ONIG_SYN_OP2_QMARK_TILDE_ABSENT_GROUP
        | ONIG_SYN_OP2_ESC_X_Y_TEXT_SEGMENT
        | ONIG_SYN_OP2_ESC_CAPITAL_R_GENERAL_NEWLINE
        | ONIG_SYN_OP2_ESC_CAPITAL_K_KEEP
        | ONIG_SYN_OP2_ESC_G_SUBEXP_CALL
        | ONIG_SYN_OP2_ESC_P_BRACE_CHAR_PROPERTY
        | ONIG_SYN_OP2_ESC_P_BRACE_CIRCUMFLEX_NOT
        | ONIG_SYN_OP2_PLUS_POSSESSIVE_REPEAT
        | ONIG_SYN_OP2_CCLASS_SET_OP
        | ONIG_SYN_OP2_ESC_CAPITAL_C_BAR_CONTROL
        | ONIG_SYN_OP2_ESC_CAPITAL_M_BAR_META
        | ONIG_SYN_OP2_ESC_V_VTAB
        | ONIG_SYN_OP2_ESC_H_XDIGIT
        | ONIG_SYN_OP2_ESC_U_HEX4,
    behavior: SYN_GNU_REGEX_BV
        | ONIG_SYN_ALLOW_INTERVAL_LOW_ABBREV
        | ONIG_SYN_DIFFERENT_LEN_ALT_LOOK_BEHIND
        | ONIG_SYN_CAPTURE_ONLY_NAMED_GROUP
        | ONIG_SYN_ALLOW_MULTIPLEX_DEFINITION_NAME
        | ONIG_SYN_FIXED_INTERVAL_IS_GREEDY_ONLY
        | ONIG_SYN_WARN_CC_OP_NOT_ESCAPED
        | ONIG_SYN_WARN_REDUNDANT_NESTED_REPEAT,
    options: ONIG_OPTION_NONE,
};

/* ===== errors ===== */

pub const ONIG_NORMAL: i32 = 0;
pub const ONIG_MISMATCH: i32 = -1;
pub const ONIG_ABORT: i32 = -3;
pub const ONIGERR_MEMORY: i32 = -5;
pub const ONIGERR_TYPE_BUG: i32 = -6;
pub const ONIGERR_PARSER_BUG: i32 = -11;
pub const ONIGERR_STACK_BUG: i32 = -12;
pub const ONIGERR_UNDEFINED_BYTECODE: i32 = -13;
pub const ONIGERR_UNEXPECTED_BYTECODE: i32 = -14;
pub const ONIGERR_MATCH_STACK_LIMIT_OVER: i32 = -15;
pub const ONIGERR_PARSE_DEPTH_LIMIT_OVER: i32 = -16;
pub const ONIGERR_RETRY_LIMIT_IN_MATCH_OVER: i32 = -17;
pub const ONIGERR_RETRY_LIMIT_IN_SEARCH_OVER: i32 = -18;
pub const ONIGERR_SUBEXP_CALL_LIMIT_IN_SEARCH_OVER: i32 = -19;
pub const ONIGERR_DEFAULT_ENCODING_IS_NOT_SET: i32 = -21;
pub const ONIGERR_SPECIFIED_ENCODING_CANT_CONVERT_TO_WIDE_CHAR: i32 = -22;
pub const ONIGERR_FAIL_TO_INITIALIZE: i32 = -23;
pub const ONIGERR_INVALID_ARGUMENT: i32 = -30;
pub const ONIGERR_END_PATTERN_AT_LEFT_BRACE: i32 = -100;
pub const ONIGERR_END_PATTERN_AT_LEFT_BRACKET: i32 = -101;
pub const ONIGERR_EMPTY_CHAR_CLASS: i32 = -102;
pub const ONIGERR_PREMATURE_END_OF_CHAR_CLASS: i32 = -103;
pub const ONIGERR_END_PATTERN_AT_ESCAPE: i32 = -104;
pub const ONIGERR_END_PATTERN_AT_META: i32 = -105;
pub const ONIGERR_END_PATTERN_AT_CONTROL: i32 = -106;
pub const ONIGERR_META_CODE_SYNTAX: i32 = -108;
pub const ONIGERR_CONTROL_CODE_SYNTAX: i32 = -109;
pub const ONIGERR_CHAR_CLASS_VALUE_AT_END_OF_RANGE: i32 = -110;
pub const ONIGERR_CHAR_CLASS_VALUE_AT_START_OF_RANGE: i32 = -111;
pub const ONIGERR_UNMATCHED_RANGE_SPECIFIER_IN_CHAR_CLASS: i32 = -112;
pub const ONIGERR_TARGET_OF_REPEAT_OPERATOR_NOT_SPECIFIED: i32 = -113;
pub const ONIGERR_TARGET_OF_REPEAT_OPERATOR_INVALID: i32 = -114;
pub const ONIGERR_NESTED_REPEAT_OPERATOR: i32 = -115;
pub const ONIGERR_UNMATCHED_CLOSE_PARENTHESIS: i32 = -116;
pub const ONIGERR_END_PATTERN_WITH_UNMATCHED_PARENTHESIS: i32 = -117;
pub const ONIGERR_END_PATTERN_IN_GROUP: i32 = -118;
pub const ONIGERR_UNDEFINED_GROUP_OPTION: i32 = -119;
pub const ONIGERR_INVALID_GROUP_OPTION: i32 = -120;
pub const ONIGERR_INVALID_POSIX_BRACKET_TYPE: i32 = -121;
pub const ONIGERR_INVALID_LOOK_BEHIND_PATTERN: i32 = -122;
pub const ONIGERR_INVALID_REPEAT_RANGE_PATTERN: i32 = -123;
pub const ONIGERR_TOO_BIG_NUMBER: i32 = -200;
pub const ONIGERR_TOO_BIG_NUMBER_FOR_REPEAT_RANGE: i32 = -201;
pub const ONIGERR_UPPER_SMALLER_THAN_LOWER_IN_REPEAT_RANGE: i32 = -202;
pub const ONIGERR_EMPTY_RANGE_IN_CHAR_CLASS: i32 = -203;
pub const ONIGERR_MISMATCH_CODE_LENGTH_IN_CLASS_RANGE: i32 = -204;
pub const ONIGERR_TOO_MANY_MULTI_BYTE_RANGES: i32 = -205;
pub const ONIGERR_TOO_SHORT_MULTI_BYTE_STRING: i32 = -206;
pub const ONIGERR_TOO_BIG_BACKREF_NUMBER: i32 = -207;
pub const ONIGERR_INVALID_BACKREF: i32 = -208;
pub const ONIGERR_NUMBERED_BACKREF_OR_CALL_NOT_ALLOWED: i32 = -209;
pub const ONIGERR_TOO_MANY_CAPTURES: i32 = -210;
pub const ONIGERR_TOO_LONG_WIDE_CHAR_VALUE: i32 = -212;
pub const ONIGERR_UNDEFINED_OPERATOR: i32 = -213;
pub const ONIGERR_EMPTY_GROUP_NAME: i32 = -214;
pub const ONIGERR_INVALID_GROUP_NAME: i32 = -215;
pub const ONIGERR_INVALID_CHAR_IN_GROUP_NAME: i32 = -216;
pub const ONIGERR_UNDEFINED_NAME_REFERENCE: i32 = -217;
pub const ONIGERR_UNDEFINED_GROUP_REFERENCE: i32 = -218;
pub const ONIGERR_MULTIPLEX_DEFINED_NAME: i32 = -219;
pub const ONIGERR_MULTIPLEX_DEFINITION_NAME_CALL: i32 = -220;
pub const ONIGERR_NEVER_ENDING_RECURSION: i32 = -221;
pub const ONIGERR_GROUP_NUMBER_OVER_FOR_CAPTURE_HISTORY: i32 = -222;
pub const ONIGERR_INVALID_CHAR_PROPERTY_NAME: i32 = -223;
pub const ONIGERR_INVALID_IF_ELSE_SYNTAX: i32 = -224;
pub const ONIGERR_INVALID_ABSENT_GROUP_PATTERN: i32 = -225;
pub const ONIGERR_INVALID_ABSENT_GROUP_GENERATOR_PATTERN: i32 = -226;
pub const ONIGERR_INVALID_CALLOUT_PATTERN: i32 = -227;
pub const ONIGERR_INVALID_CALLOUT_NAME: i32 = -228;
pub const ONIGERR_UNDEFINED_CALLOUT_NAME: i32 = -229;
pub const ONIGERR_INVALID_CALLOUT_BODY: i32 = -230;
pub const ONIGERR_INVALID_CALLOUT_TAG_NAME: i32 = -231;
pub const ONIGERR_INVALID_CALLOUT_ARG: i32 = -232;
pub const ONIGERR_INVALID_CODE_POINT_VALUE: i32 = -400;
pub const ONIGERR_INVALID_WIDE_CHAR_VALUE: i32 = -400;
pub const ONIGERR_TOO_BIG_WIDE_CHAR_VALUE: i32 = -401;
pub const ONIGERR_NOT_SUPPORTED_ENCODING_COMBINATION: i32 = -402;
pub const ONIGERR_INVALID_COMBINATION_OF_OPTIONS: i32 = -403;
pub const ONIGERR_TOO_MANY_USER_DEFINED_OBJECTS: i32 = -404;
pub const ONIGERR_TOO_LONG_PROPERTY_NAME: i32 = -405;
pub const ONIGERR_VERY_INEFFICIENT_PATTERN: i32 = -406;
pub const ONIGERR_LIBRARY_IS_NOT_INITIALIZED: i32 = -500;

/* ===== limits ===== */

pub const ONIG_MAX_CAPTURE_NUM: i32 = 2147483647;
pub const ONIG_MAX_BACKREF_NUM: i32 = 1000;
pub const ONIG_MAX_REPEAT_NUM: i32 = 100000;
pub const ONIG_MAX_MULTI_BYTE_RANGES_NUM: usize = 10000;
pub const ONIG_MAX_CAPTURE_HISTORY_GROUP: i32 = 31;
pub const DEFAULT_MAX_CAPTURE_NUM: i32 = 32767;
pub const DEFAULT_PARSE_DEPTH_LIMIT: u32 = 4096;
pub const INFINITE_REPEAT: i32 = -1;
pub const INFINITE_LEN: u32 = !0u32;
pub const ONIG_INT_MAX: i32 = i32::MAX;
pub const SINGLE_BYTE_SIZE: u32 = 256;
pub const CHAR_MAP_SIZE: usize = 256;
pub const STEP_BACK_MAX_CHAR_LEN: u32 = 65535;
pub const LOOK_BEHIND_MAX_CHAR_LEN: u32 = STEP_BACK_MAX_CHAR_LEN;
pub const DEFAULT_SUBEXP_CALL_MAX_NEST_LEVEL: i32 = 20;

#[inline]
pub fn is_infinite_repeat(n: i32) -> bool {
    n == INFINITE_REPEAT
}

/* ===== mem status (bit sets) ===== */

pub type MemStatusType = u32;
pub const MEM_STATUS_BITS_NUM: i32 = 32;

#[inline]
pub fn mem_status_at(stats: u32, n: i32) -> bool {
    if n < MEM_STATUS_BITS_NUM {
        stats & (1u32 << n) != 0
    } else {
        stats & 1 != 0
    }
}
#[inline]
pub fn mem_status_at0(stats: u32, n: i32) -> bool {
    if n > 0 && n < MEM_STATUS_BITS_NUM {
        stats & (1u32 << n) != 0
    } else {
        stats & 1 != 0
    }
}
#[inline]
pub fn mem_status_is_all_on(stats: u32) -> bool {
    stats & 1 != 0
}
#[inline]
pub fn mem_status_on(stats: &mut u32, n: i32) {
    if n < MEM_STATUS_BITS_NUM {
        if n != 0 {
            *stats |= 1u32 << n;
        }
    } else {
        *stats |= 1;
    }
}
#[inline]
pub fn mem_status_on_simple(stats: &mut u32, n: i32) {
    if n < MEM_STATUS_BITS_NUM {
        *stats |= 1u32 << n;
    }
}
#[inline]
pub fn mem_status_limit_at(stats: u32, n: i32) -> bool {
    if n < MEM_STATUS_BITS_NUM {
        stats & (1u32 << n) != 0
    } else {
        false
    }
}
#[inline]
pub fn mem_status_limit_on(stats: &mut u32, n: i32) {
    if n < MEM_STATUS_BITS_NUM && n != 0 {
        *stats |= 1u32 << n;
    }
}

/* ===== bitset ===== */

pub type BitSet = [u32; 8];

#[inline]
pub fn bitset_at(bs: &BitSet, pos: u32) -> bool {
    bs[(pos >> 5) as usize] & (1u32 << (pos & 0x1f)) != 0
}
#[inline]
pub fn bitset_set_bit(bs: &mut BitSet, pos: u32) {
    bs[(pos >> 5) as usize] |= 1u32 << (pos & 0x1f);
}
#[inline]
pub fn bitset_clear_bit(bs: &mut BitSet, pos: u32) {
    bs[(pos >> 5) as usize] &= !(1u32 << (pos & 0x1f));
}
pub fn bitset_is_empty(bs: &BitSet) -> bool {
    bs.iter().all(|&x| x == 0)
}

/* ===== anchors ===== */

pub const ANCR_PREC_READ: i32 = 1 << 0;
pub const ANCR_PREC_READ_NOT: i32 = 1 << 1;
pub const ANCR_LOOK_BEHIND: i32 = 1 << 2;
pub const ANCR_LOOK_BEHIND_NOT: i32 = 1 << 3;
pub const ANCR_BEGIN_BUF: i32 = 1 << 4;
pub const ANCR_BEGIN_LINE: i32 = 1 << 5;
pub const ANCR_BEGIN_POSITION: i32 = 1 << 6;
pub const ANCR_END_BUF: i32 = 1 << 7;
pub const ANCR_SEMI_END_BUF: i32 = 1 << 8;
pub const ANCR_END_LINE: i32 = 1 << 9;
pub const ANCR_WORD_BOUNDARY: i32 = 1 << 10;
pub const ANCR_NO_WORD_BOUNDARY: i32 = 1 << 11;
pub const ANCR_WORD_BEGIN: i32 = 1 << 12;
pub const ANCR_WORD_END: i32 = 1 << 13;
pub const ANCR_ANYCHAR_INF: i32 = 1 << 14;
pub const ANCR_ANYCHAR_INF_ML: i32 = 1 << 15;
pub const ANCR_TEXT_SEGMENT_BOUNDARY: i32 = 1 << 16;
pub const ANCR_NO_TEXT_SEGMENT_BOUNDARY: i32 = 1 << 17;
pub const ANCR_ANYCHAR_INF_MASK: i32 = ANCR_ANYCHAR_INF | ANCR_ANYCHAR_INF_ML;
pub const ANCR_END_BUF_MASK: i32 = ANCR_END_BUF | ANCR_SEMI_END_BUF;

#[inline]
pub fn is_word_anchor_type(t: i32) -> bool {
    t == ANCR_WORD_BOUNDARY
        || t == ANCR_NO_WORD_BOUNDARY
        || t == ANCR_WORD_BEGIN
        || t == ANCR_WORD_END
}

/* ===== nodes ===== */

pub type NodeRef = usize;
pub const NULL_NODE: NodeRef = usize::MAX;

pub const CTYPE_ANYCHAR: i32 = -1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeType {
    String = 0,
    CClass = 1,
    CType = 2,
    BackRef = 3,
    Quant = 4,
    Bag = 5,
    Anchor = 6,
    List = 7,
    Alt = 8,
    Call = 9,
    Gimmick = 10,
}

#[inline]
pub fn nd_type2bit(t: NodeType) -> u32 {
    1 << (t as u32)
}
pub const ND_BIT_STRING: u32 = 1 << 0;
pub const ND_BIT_CCLASS: u32 = 1 << 1;
pub const ND_BIT_CTYPE: u32 = 1 << 2;
pub const ND_BIT_BACKREF: u32 = 1 << 3;
pub const ND_BIT_QUANT: u32 = 1 << 4;
pub const ND_BIT_BAG: u32 = 1 << 5;
pub const ND_BIT_ANCHOR: u32 = 1 << 6;
pub const ND_BIT_LIST: u32 = 1 << 7;
pub const ND_BIT_ALT: u32 = 1 << 8;
pub const ND_BIT_CALL: u32 = 1 << 9;
pub const ND_BIT_GIMMICK: u32 = 1 << 10;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BagType {
    Memory = 0,
    Option = 1,
    StopBacktrack = 2,
    IfElse = 3,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GimmickType {
    Fail = 0,
    Save = 1,
    UpdateVar = 2,
    Callout = 3,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum BodyEmpty {
    IsNotEmpty = 0,
    MayBeEmpty = 1,
    MayBeEmptyMem = 2,
    MayBeEmptyRec = 3,
}

pub const ND_STRING_CRUDE: u32 = 1 << 0;
pub const ND_STRING_CASE_EXPANDED: u32 = 1 << 1;

/* node status bits */
pub const ND_ST_FIXED_MIN: u32 = 1 << 0;
pub const ND_ST_FIXED_MAX: u32 = 1 << 1;
pub const ND_ST_FIXED_CLEN: u32 = 1 << 2;
pub const ND_ST_MARK1: u32 = 1 << 3;
pub const ND_ST_MARK2: u32 = 1 << 4;
pub const ND_ST_STRICT_REAL_REPEAT: u32 = 1 << 5;
pub const ND_ST_RECURSION: u32 = 1 << 6;
pub const ND_ST_CALLED: u32 = 1 << 7;
pub const ND_ST_FIXED_ADDR: u32 = 1 << 8;
pub const ND_ST_NAMED_GROUP: u32 = 1 << 9;
pub const ND_ST_IN_REAL_REPEAT: u32 = 1 << 10;
pub const ND_ST_IN_ZERO_REPEAT: u32 = 1 << 11;
pub const ND_ST_IN_MULTI_ENTRY: u32 = 1 << 12;
pub const ND_ST_NEST_LEVEL: u32 = 1 << 13;
pub const ND_ST_BY_NUMBER: u32 = 1 << 14;
pub const ND_ST_BY_NAME: u32 = 1 << 15;
pub const ND_ST_BACKREF: u32 = 1 << 16;
pub const ND_ST_CHECKER: u32 = 1 << 17;
pub const ND_ST_PROHIBIT_RECURSION: u32 = 1 << 18;
pub const ND_ST_SUPER: u32 = 1 << 19;
pub const ND_ST_EMPTY_STATUS_CHECK: u32 = 1 << 20;
pub const ND_ST_IGNORECASE: u32 = 1 << 21;
pub const ND_ST_MULTILINE: u32 = 1 << 22;
pub const ND_ST_TEXT_SEGMENT_WORD: u32 = 1 << 23;
pub const ND_ST_ABSENT_WITH_SIDE_EFFECTS: u32 = 1 << 24;
pub const ND_ST_FIXED_CLEN_MIN_SURE: u32 = 1 << 25;
pub const ND_ST_REFERENCED: u32 = 1 << 26;
pub const ND_ST_INPEEK: u32 = 1 << 27;
pub const ND_ST_WHOLE_OPTIONS: u32 = 1 << 28;

/// CClassNode
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CClass {
    pub not: bool,
    pub bs: BitSet,
    /// multi-byte ranges (None = C's NULL mbuf)
    pub mbuf: Option<Vec<(u32, u32)>>,
}

#[derive(Clone, Debug)]
pub enum NodeKind {
    Str {
        s: Vec<u8>,
        flag: u32,
    },
    CClass(Box<CClass>),
    CType {
        ctype: i32,
        not: bool,
        ascii_mode: bool,
    },
    BackRef {
        back: Vec<i32>,
        nest_level: i32,
    },
    Quant {
        body: NodeRef,
        lower: i32,
        upper: i32,
        greedy: bool,
        emptiness: BodyEmpty,
        head_exact: NodeRef,
        next_head_exact: NodeRef,
        include_referred: bool,
        empty_status_mem: MemStatusType,
    },
    Bag {
        body: NodeRef,
        typ: BagType,
        /* BAG_MEMORY */
        regnum: i32,
        called_addr: i32,
        entry_count: i32,
        called_state: i32,
        /* BAG_OPTION */
        options: u32,
        /* BAG_IF_ELSE */
        then_: NodeRef,
        else_: NodeRef,
        /* for multiple call reference */
        min_len: u32,
        max_len: u32,
        min_char_len: u32,
        max_char_len: u32,
        opt_count: i32,
    },
    Anchor {
        body: NodeRef,
        typ: i32,
        char_min_len: u32,
        char_max_len: u32,
        ascii_mode: bool,
        lead_node: NodeRef,
    },
    List {
        car: NodeRef,
        cdr: NodeRef,
    },
    Alt {
        car: NodeRef,
        cdr: NodeRef,
    },
    Call {
        body: NodeRef,
        by_number: bool,
        called_gnum: i32,
        name: Vec<u8>,
        entry_count: i32,
    },
    Gimmick {
        typ: GimmickType,
        detail_type: i32,
        num: i32,
        id: i32,
    },
}

#[derive(Clone, Debug)]
pub struct Node {
    pub kind: NodeKind,
    pub status: u32,
    pub parent: NodeRef,
}

/// Arena holding every node of a pattern tree (C's malloc'ed nodes).
/// Freed nodes simply stay unused in the arena.
#[derive(Default, Debug)]
pub struct Nodes {
    pub v: Vec<Node>,
}

impl Nodes {
    pub fn alloc(&mut self, kind: NodeKind) -> NodeRef {
        self.v.push(Node {
            kind,
            status: 0,
            parent: NULL_NODE,
        });
        self.v.len() - 1
    }
    #[inline]
    pub fn typ(&self, n: NodeRef) -> NodeType {
        match &self.v[n].kind {
            NodeKind::Str { .. } => NodeType::String,
            NodeKind::CClass(_) => NodeType::CClass,
            NodeKind::CType { .. } => NodeType::CType,
            NodeKind::BackRef { .. } => NodeType::BackRef,
            NodeKind::Quant { .. } => NodeType::Quant,
            NodeKind::Bag { .. } => NodeType::Bag,
            NodeKind::Anchor { .. } => NodeType::Anchor,
            NodeKind::List { .. } => NodeType::List,
            NodeKind::Alt { .. } => NodeType::Alt,
            NodeKind::Call { .. } => NodeType::Call,
            NodeKind::Gimmick { .. } => NodeType::Gimmick,
        }
    }
    #[inline]
    pub fn status(&self, n: NodeRef) -> u32 {
        self.v[n].status
    }
    #[inline]
    pub fn has(&self, n: NodeRef, f: u32) -> bool {
        self.v[n].status & f != 0
    }
    #[inline]
    pub fn add(&mut self, n: NodeRef, f: u32) {
        self.v[n].status |= f;
    }
    #[inline]
    pub fn remove(&mut self, n: NodeRef, f: u32) {
        self.v[n].status &= !f;
    }
    #[inline]
    pub fn parent(&self, n: NodeRef) -> NodeRef {
        self.v[n].parent
    }
    #[inline]
    pub fn set_parent(&mut self, n: NodeRef, p: NodeRef) {
        self.v[n].parent = p;
    }
    /// ND_BODY (quant, bag, anchor, call)
    #[inline]
    pub fn body(&self, n: NodeRef) -> NodeRef {
        match &self.v[n].kind {
            NodeKind::Quant { body, .. }
            | NodeKind::Bag { body, .. }
            | NodeKind::Anchor { body, .. }
            | NodeKind::Call { body, .. } => *body,
            _ => NULL_NODE,
        }
    }
    #[inline]
    pub fn set_body(&mut self, n: NodeRef, b: NodeRef) {
        match &mut self.v[n].kind {
            NodeKind::Quant { body, .. }
            | NodeKind::Bag { body, .. }
            | NodeKind::Anchor { body, .. }
            | NodeKind::Call { body, .. } => *body = b,
            _ => panic!("set_body on a node without body"),
        }
    }
    #[inline]
    pub fn car(&self, n: NodeRef) -> NodeRef {
        match &self.v[n].kind {
            NodeKind::List { car, .. } | NodeKind::Alt { car, .. } => *car,
            _ => panic!("car on a non-cons node"),
        }
    }
    #[inline]
    pub fn cdr(&self, n: NodeRef) -> NodeRef {
        match &self.v[n].kind {
            NodeKind::List { cdr, .. } | NodeKind::Alt { cdr, .. } => *cdr,
            _ => panic!("cdr on a non-cons node"),
        }
    }
    #[inline]
    pub fn set_car(&mut self, n: NodeRef, x: NodeRef) {
        match &mut self.v[n].kind {
            NodeKind::List { car, .. } | NodeKind::Alt { car, .. } => *car = x,
            _ => panic!("set_car on a non-cons node"),
        }
    }
    #[inline]
    pub fn set_cdr(&mut self, n: NodeRef, x: NodeRef) {
        match &mut self.v[n].kind {
            NodeKind::List { cdr, .. } | NodeKind::Alt { cdr, .. } => *cdr = x,
            _ => panic!("set_cdr on a non-cons node"),
        }
    }
    #[inline]
    pub fn is_cons(&self, n: NodeRef) -> bool {
        matches!(self.v[n].kind, NodeKind::List { .. } | NodeKind::Alt { .. })
    }
    pub fn str_bytes(&self, n: NodeRef) -> &[u8] {
        match &self.v[n].kind {
            NodeKind::Str { s, .. } => s,
            _ => panic!("str_bytes on a non-string node"),
        }
    }
    pub fn str_flag(&self, n: NodeRef) -> u32 {
        match &self.v[n].kind {
            NodeKind::Str { flag, .. } => *flag,
            _ => panic!("str_flag on a non-string node"),
        }
    }
    pub fn str_is_crude(&self, n: NodeRef) -> bool {
        self.str_flag(n) & ND_STRING_CRUDE != 0
    }
    pub fn str_mut(&mut self, n: NodeRef) -> &mut Vec<u8> {
        match &mut self.v[n].kind {
            NodeKind::Str { s, .. } => s,
            _ => panic!("str_mut on a non-string node"),
        }
    }
    pub fn cclass(&self, n: NodeRef) -> &CClass {
        match &self.v[n].kind {
            NodeKind::CClass(cc) => cc,
            _ => panic!("cclass on a non-cclass node"),
        }
    }
    pub fn cclass_mut(&mut self, n: NodeRef) -> &mut CClass {
        match &mut self.v[n].kind {
            NodeKind::CClass(cc) => cc,
            _ => panic!("cclass_mut on a non-cclass node"),
        }
    }
    /// node_swap
    pub fn swap(&mut self, a: NodeRef, b: NodeRef) {
        self.v.swap(a, b);
    }
    /// `*a = *b` (struct copy)
    pub fn copy_into(&mut self, a: NodeRef, b: NodeRef) {
        self.v[a] = self.v[b].clone();
    }
    pub fn is_anychar(&self, n: NodeRef) -> bool {
        matches!(
            self.v[n].kind,
            NodeKind::CType {
                ctype: CTYPE_ANYCHAR,
                ..
            }
        )
    }
}

/* ===== name table ===== */

#[derive(Clone, Debug)]
pub struct NameEntry {
    pub name: Vec<u8>,
    pub back_refs: Vec<i32>,
}

/// Oniguruma keeps names in an st hash table; foreach visits entries in
/// bin order (11 bins initially, entries prepended to their bin, rehashed
/// when the density exceeds 5). We store entries in insertion order and
/// reproduce the visit order on demand.
#[derive(Clone, Debug, Default)]
pub struct NameTable {
    pub entries: Vec<NameEntry>,
}

impl NameTable {
    pub fn find(&self, name: &[u8]) -> Option<usize> {
        self.entries.iter().position(|e| e.name == name)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Entry indices in onig_foreach_name order
    pub fn foreach_order(&self) -> Vec<usize> {
        fn hash(s: &[u8]) -> u32 {
            let mut val: u32 = 0;
            for &b in s {
                val = val.wrapping_mul(997).wrapping_add(b as u32);
            }
            val.wrapping_add(val >> 5)
        }
        const PRIMES: [usize; 12] = [11, 19, 37, 67, 131, 283, 521, 1033, 2053, 4099, 8219, 16427];
        fn new_size(size: usize) -> usize {
            let mut ns = 8;
            for p in PRIMES {
                if ns > size {
                    return p;
                }
                ns <<= 1;
            }
            size * 2 + 1
        }
        let mut num_bins = new_size(5);
        let mut bins: Vec<Vec<usize>> = vec![Vec::new(); num_bins];
        for (i, e) in self.entries.iter().enumerate() {
            let h = hash(&e.name) as usize;
            // `i` is the number of entries already inserted
            if i / num_bins > 5 {
                let nb = new_size(num_bins + 1);
                let mut newbins: Vec<Vec<usize>> = vec![Vec::new(); nb];
                for bin in &bins {
                    for &x in bin.iter() {
                        let hh = hash(&self.entries[x].name) as usize % nb;
                        newbins[hh].insert(0, x);
                    }
                }
                bins = newbins;
                num_bins = nb;
            }
            bins[h % num_bins].insert(0, i);
        }
        bins.into_iter().flatten().collect()
    }
}

/* ===== callouts (only what is reachable without registered callouts) ===== */

#[derive(Clone, Debug, Default)]
pub struct CalloutListEntry {
    pub of_contents: bool,
    pub tag: Option<Vec<u8>>,
}

pub fn encoding_is(enc: &Encoding, other: &Encoding) -> bool {
    std::ptr::eq(enc, other)
}
