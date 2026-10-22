//! intl_error.c: the global error (INTL_G(g_error)) and the per-object
//! errors of the intl classes, ICU's UErrorCode names (u_errorName()) and
//! the intl.error_level / intl.use_exceptions reporting.

use std::cell::RefCell;

use goro_core::vm::Vm;

pub const U_ZERO_ERROR: i32 = 0;
pub const U_USING_FALLBACK_WARNING: i32 = -128;
pub const U_USING_DEFAULT_WARNING: i32 = -127;
pub const U_STRING_NOT_TERMINATED_WARNING: i32 = -124;
pub const U_ILLEGAL_ARGUMENT_ERROR: i32 = 1;
pub const U_MISSING_RESOURCE_ERROR: i32 = 2;
pub const U_INVALID_FORMAT_ERROR: i32 = 3;
pub const U_FILE_ACCESS_ERROR: i32 = 4;
pub const U_INTERNAL_PROGRAM_ERROR: i32 = 5;
pub const U_MESSAGE_PARSE_ERROR: i32 = 6;
pub const U_MEMORY_ALLOCATION_ERROR: i32 = 7;
pub const U_INDEX_OUTOFBOUNDS_ERROR: i32 = 8;
pub const U_PARSE_ERROR: i32 = 9;
pub const U_INVALID_CHAR_FOUND: i32 = 10;
pub const U_TRUNCATED_CHAR_FOUND: i32 = 11;
pub const U_ILLEGAL_CHAR_FOUND: i32 = 12;
pub const U_BUFFER_OVERFLOW_ERROR: i32 = 15;
pub const U_UNSUPPORTED_ERROR: i32 = 16;
pub const U_RESOURCE_TYPE_MISMATCH: i32 = 17;
pub const U_ILLEGAL_ESCAPE_SEQUENCE: i32 = 18;
pub const U_INVALID_STATE_ERROR: i32 = 27;
pub const U_INVALID_ID: i32 = 65569;
pub const U_UNEXPECTED_TOKEN: i32 = 65792;
pub const U_PATTERN_SYNTAX_ERROR: i32 = 65799;
pub const U_UNMATCHED_BRACES: i32 = 65801;
pub const U_ARGUMENT_TYPE_MISMATCH: i32 = 65804;
pub const U_DUPLICATE_KEYWORD: i32 = 65805;
pub const U_UNDEFINED_KEYWORD: i32 = 65806;
pub const U_DEFAULT_KEYWORD_MISSING: i32 = 65807;
pub const U_DECIMAL_NUMBER_SYNTAX_ERROR: i32 = 65808;
pub const U_FORMAT_INEXACT_ERROR: i32 = 65809;
pub const U_BRK_RULE_SYNTAX: i32 = 66051;
pub const U_IDNA_ERROR_START: i32 = 66560;

/// U_FAILURE()
pub fn failure(code: i32) -> bool {
    code > U_ZERO_ERROR
}

const WARNINGS: &[&str] = &[
    "U_USING_FALLBACK_WARNING",
    "U_USING_DEFAULT_WARNING",
    "U_SAFECLONE_ALLOCATED_WARNING",
    "U_STATE_OLD_WARNING",
    "U_STRING_NOT_TERMINATED_WARNING",
    "U_SORT_KEY_TOO_SHORT_WARNING",
    "U_AMBIGUOUS_ALIAS_WARNING",
    "U_DIFFERENT_UCA_VERSION",
    "U_PLUGIN_CHANGED_LEVEL_WARNING",
];

const STANDARD: &[&str] = &[
    "U_ZERO_ERROR",
    "U_ILLEGAL_ARGUMENT_ERROR",
    "U_MISSING_RESOURCE_ERROR",
    "U_INVALID_FORMAT_ERROR",
    "U_FILE_ACCESS_ERROR",
    "U_INTERNAL_PROGRAM_ERROR",
    "U_MESSAGE_PARSE_ERROR",
    "U_MEMORY_ALLOCATION_ERROR",
    "U_INDEX_OUTOFBOUNDS_ERROR",
    "U_PARSE_ERROR",
    "U_INVALID_CHAR_FOUND",
    "U_TRUNCATED_CHAR_FOUND",
    "U_ILLEGAL_CHAR_FOUND",
    "U_INVALID_TABLE_FORMAT",
    "U_INVALID_TABLE_FILE",
    "U_BUFFER_OVERFLOW_ERROR",
    "U_UNSUPPORTED_ERROR",
    "U_RESOURCE_TYPE_MISMATCH",
    "U_ILLEGAL_ESCAPE_SEQUENCE",
    "U_UNSUPPORTED_ESCAPE_SEQUENCE",
    "U_NO_SPACE_AVAILABLE",
    "U_CE_NOT_FOUND_ERROR",
    "U_PRIMARY_TOO_LONG_ERROR",
    "U_STATE_TOO_OLD_ERROR",
    "U_TOO_MANY_ALIASES_ERROR",
    "U_ENUM_OUT_OF_SYNC_ERROR",
    "U_INVARIANT_CONVERSION_ERROR",
    "U_INVALID_STATE_ERROR",
    "U_COLLATOR_VERSION_MISMATCH",
    "U_USELESS_COLLATOR_ERROR",
    "U_NO_WRITE_PERMISSION",
    "U_INPUT_TOO_LONG_ERROR",
];

const TRANSLIT: &[&str] = &[
    "U_BAD_VARIABLE_DEFINITION",
    "U_MALFORMED_RULE",
    "U_MALFORMED_SET",
    "U_MALFORMED_SYMBOL_REFERENCE",
    "U_MALFORMED_UNICODE_ESCAPE",
    "U_MALFORMED_VARIABLE_DEFINITION",
    "U_MALFORMED_VARIABLE_REFERENCE",
    "U_MISMATCHED_SEGMENT_DELIMITERS",
    "U_MISPLACED_ANCHOR_START",
    "U_MISPLACED_CURSOR_OFFSET",
    "U_MISPLACED_QUANTIFIER",
    "U_MISSING_OPERATOR",
    "U_MISSING_SEGMENT_CLOSE",
    "U_MULTIPLE_ANTE_CONTEXTS",
    "U_MULTIPLE_CURSORS",
    "U_MULTIPLE_POST_CONTEXTS",
    "U_TRAILING_BACKSLASH",
    "U_UNDEFINED_SEGMENT_REFERENCE",
    "U_UNDEFINED_VARIABLE",
    "U_UNQUOTED_SPECIAL",
    "U_UNTERMINATED_QUOTE",
    "U_RULE_MASK_ERROR",
    "U_MISPLACED_COMPOUND_FILTER",
    "U_MULTIPLE_COMPOUND_FILTERS",
    "U_INVALID_RBT_SYNTAX",
    "U_INVALID_PROPERTY_PATTERN",
    "U_MALFORMED_PRAGMA",
    "U_UNCLOSED_SEGMENT",
    "U_ILLEGAL_CHAR_IN_SEGMENT",
    "U_VARIABLE_RANGE_EXHAUSTED",
    "U_VARIABLE_RANGE_OVERLAP",
    "U_ILLEGAL_CHARACTER",
    "U_INTERNAL_TRANSLITERATOR_ERROR",
    "U_INVALID_ID",
    "U_INVALID_FUNCTION",
];

const FMT: &[&str] = &[
    "U_UNEXPECTED_TOKEN",
    "U_MULTIPLE_DECIMAL_SEPARATORS",
    "U_MULTIPLE_EXPONENTIAL_SYMBOLS",
    "U_MALFORMED_EXPONENTIAL_PATTERN",
    "U_MULTIPLE_PERCENT_SYMBOLS",
    "U_MULTIPLE_PERMILL_SYMBOLS",
    "U_MULTIPLE_PAD_SPECIFIERS",
    "U_PATTERN_SYNTAX_ERROR",
    "U_ILLEGAL_PAD_POSITION",
    "U_UNMATCHED_BRACES",
    "U_UNSUPPORTED_PROPERTY",
    "U_UNSUPPORTED_ATTRIBUTE",
    "U_ARGUMENT_TYPE_MISMATCH",
    "U_DUPLICATE_KEYWORD",
    "U_UNDEFINED_KEYWORD",
    "U_DEFAULT_KEYWORD_MISSING",
    "U_DECIMAL_NUMBER_SYNTAX_ERROR",
    "U_FORMAT_INEXACT_ERROR",
    "U_NUMBER_ARG_OUTOFBOUNDS_ERROR",
    "U_NUMBER_SKELETON_SYNTAX_ERROR",
    "U_MF_UNRESOLVED_VARIABLE_ERROR",
    "U_MF_SYNTAX_ERROR",
    "U_MF_UNKNOWN_FUNCTION_ERROR",
    "U_MF_VARIANT_KEY_MISMATCH_ERROR",
    "U_MF_FORMATTING_ERROR",
    "U_MF_NONEXHAUSTIVE_PATTERN_ERROR",
    "U_MF_DUPLICATE_OPTION_NAME_ERROR",
    "U_MF_SELECTOR_ERROR",
    "U_MF_MISSING_SELECTOR_ANNOTATION_ERROR",
    "U_MF_DUPLICATE_DECLARATION_ERROR",
    "U_MF_OPERAND_MISMATCH_ERROR",
    "U_MF_DUPLICATE_VARIANT_ERROR",
    "U_MF_BAD_OPTION",
];

const BRK: &[&str] = &[
    "U_BRK_INTERNAL_ERROR",
    "U_BRK_HEX_DIGITS_EXPECTED",
    "U_BRK_SEMICOLON_EXPECTED",
    "U_BRK_RULE_SYNTAX",
    "U_BRK_UNCLOSED_SET",
    "U_BRK_ASSIGN_ERROR",
    "U_BRK_VARIABLE_REDFINITION",
    "U_BRK_MISMATCHED_PAREN",
    "U_BRK_NEW_LINE_IN_QUOTED_STRING",
    "U_BRK_UNDEFINED_VARIABLE",
    "U_BRK_INIT_ERROR",
    "U_BRK_RULE_EMPTY_SET",
    "U_BRK_UNRECOGNIZED_OPTION",
    "U_BRK_MALFORMED_RULE_TAG",
];

const REGEX: &[&str] = &[
    "U_REGEX_INTERNAL_ERROR",
    "U_REGEX_RULE_SYNTAX",
    "U_REGEX_INVALID_STATE",
    "U_REGEX_BAD_ESCAPE_SEQUENCE",
    "U_REGEX_PROPERTY_SYNTAX",
    "U_REGEX_UNIMPLEMENTED",
    "U_REGEX_MISMATCHED_PAREN",
    "U_REGEX_NUMBER_TOO_BIG",
    "U_REGEX_BAD_INTERVAL",
    "U_REGEX_MAX_LT_MIN",
    "U_REGEX_INVALID_BACK_REF",
    "U_REGEX_INVALID_FLAG",
    "U_REGEX_LOOK_BEHIND_LIMIT",
    "U_REGEX_SET_CONTAINS_STRING",
    "U_REGEX_OCTAL_TOO_BIG",
    "U_REGEX_MISSING_CLOSE_BRACKET",
    "U_REGEX_INVALID_RANGE",
    "U_REGEX_STACK_OVERFLOW",
    "U_REGEX_TIME_OUT",
    "U_REGEX_STOPPED_BY_CALLER",
    "U_REGEX_PATTERN_TOO_BIG",
    "U_REGEX_INVALID_CAPTURE_GROUP_NAME",
];

const IDNA: &[&str] = &[
    "U_STRINGPREP_PROHIBITED_ERROR",
    "U_STRINGPREP_UNASSIGNED_ERROR",
    "U_STRINGPREP_CHECK_BIDI_ERROR",
    "U_IDNA_STD3_ASCII_RULES_ERROR",
    "U_IDNA_ACE_PREFIX_ERROR",
    "U_IDNA_VERIFICATION_ERROR",
    "U_IDNA_LABEL_TOO_LONG_ERROR",
    "U_IDNA_ZERO_LENGTH_LABEL_ERROR",
    "U_IDNA_DOMAIN_NAME_TOO_LONG_ERROR",
];

const PLUGIN: &[&str] = &["U_PLUGIN_TOO_HIGH", "U_PLUGIN_DIDNT_SET_LEVEL"];

/// u_errorName()
pub fn error_name(code: i64) -> &'static str {
    let at = |t: &'static [&'static str], start: i64| -> Option<&'static str> {
        let i = code - start;
        (i >= 0 && (i as usize) < t.len()).then(|| t[i as usize])
    };
    let r = if code >= 0x10500 {
        at(PLUGIN, 0x10500)
    } else if code >= 0x10400 {
        at(IDNA, 0x10400)
    } else if code >= 0x10300 {
        at(REGEX, 0x10300)
    } else if code >= 0x10200 {
        at(BRK, 0x10200)
    } else if code >= 0x10100 {
        at(FMT, 0x10100)
    } else if code >= 0x10000 {
        at(TRANSLIT, 0x10000)
    } else if code >= 0 {
        at(STANDARD, 0)
    } else {
        at(WARNINGS, -128)
    };
    r.unwrap_or("[BOGUS UErrorCode]")
}

/// An intl_error: an ICU status and an optional message (already prefixed
/// with the function that set it).
#[derive(Clone, Debug, Default)]
pub struct IntlError {
    pub code: i32,
    pub msg: Option<String>,
}

impl IntlError {
    pub fn reset(&mut self) {
        self.code = U_ZERO_ERROR;
        self.msg = None;
    }

    /// intl_error_get_message()
    pub fn message(&self) -> String {
        let name = error_name(self.code as i64);
        match &self.msg {
            Some(m) => format!("{}: {}", m, name),
            None => name.to_string(),
        }
    }
}

thread_local! {
    static GLOBAL: RefCell<IntlError> = RefCell::new(IntlError::default());
}

pub fn global() -> IntlError {
    GLOBAL.with(|g| g.borrow().clone())
}

pub fn global_code() -> i32 {
    GLOBAL.with(|g| g.borrow().code)
}

/// intl_error_reset(NULL)
pub fn reset_global() {
    GLOBAL.with(|g| g.borrow_mut().reset());
}

/// intl_error_set_code(NULL, code)
pub fn set_global_code(code: i32) {
    GLOBAL.with(|g| g.borrow_mut().code = code);
}

/// intl.error_level
pub fn error_level(vm: &Vm) -> i64 {
    let v = vm
        .constants
        .get(b"intl.error_level".as_slice())
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default();
    let (n, _) = goro_core::ini_quantity::parse_quantity(&v, true);
    n as i64
}

/// intl.use_exceptions
pub fn use_exceptions(vm: &Vm) -> bool {
    match vm.constants.get(b"intl.use_exceptions".as_slice()) {
        Some(v) => {
            let s = v.to_php_string();
            let l = s.as_bytes().to_ascii_lowercase();
            matches!(l.as_slice(), b"on" | b"yes" | b"true")
                || std::str::from_utf8(&l)
                    .ok()
                    .and_then(|s| s.trim().parse::<i64>().ok())
                    .is_some_and(|n| n != 0)
        }
        None => false,
    }
}

/// intl_error_set_custom_msg(NULL, msg): reports the error (intl.error_level,
/// intl.use_exceptions) and stores the message in the global error. `func`
/// is the active function or method name.
fn global_set_msg(vm: &mut Vm, func: &str, msg: &str) {
    let prefixed = format!("{}(): {}", func, msg);
    let level = error_level(vm);
    if level != 0 {
        // php_error_docref() prefixes the active function itself.
        let line = vm.current_line;
        vm.raise_error(level, &prefixed, line);
    }
    if use_exceptions(vm) && vm.current_exception.is_none() {
        let e = vm.create_exception(b"IntlException", &prefixed, vm.current_line);
        vm.current_exception = Some(e);
    }
    GLOBAL.with(|g| g.borrow_mut().msg = Some(prefixed));
}

/// intl_error_set(NULL, code, msg)
pub fn set_global(vm: &mut Vm, func: &str, code: i32, msg: &str) {
    set_global_code(code);
    global_set_msg(vm, func, msg);
}

/// intl_errors_set(err, code, msg): the object's error and the global one.
pub fn set_both(vm: &mut Vm, func: &str, err: &mut IntlError, code: i32, msg: &str) {
    err.code = code;
    err.msg = Some(format!("{}(): {}", func, msg));
    set_global(vm, func, code, msg);
}

/// intl_errors_set_code(err, code)
pub fn set_both_code(err: &mut IntlError, code: i32) {
    err.code = code;
    set_global_code(code);
}

/// intl_errors_reset(err)
pub fn reset_both(err: &mut IntlError) {
    err.reset();
    reset_global();
}

/// The global error's message (intl_get_error_message()).
pub fn global_message() -> String {
    GLOBAL.with(|g| g.borrow().message())
}

/// intl_parse_error_to_string()
pub fn parse_error_string(line: i32, offset: i32, pre: &str, post: &str) -> String {
    let mut s = String::from("parse error ");
    let mut any = false;
    if line > 0 {
        s.push_str(&format!("on line {}", line));
        any = true;
    }
    if offset >= 0 {
        s.push_str(if any { ", " } else { "at " });
        s.push_str(&format!("offset {}", offset));
        any = true;
    }
    if !pre.is_empty() {
        if any {
            s.push_str(", ");
        }
        s.push_str(&format!("after \"{}\"", pre));
        any = true;
    }
    if !post.is_empty() {
        if any {
            s.push_str(", ");
        }
        s.push_str(&format!("before or at \"{}\"", post));
        any = true;
    }
    if !any {
        return "no parse error".to_string();
    }
    s
}
