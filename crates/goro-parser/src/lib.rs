pub mod ast;
pub mod lexer;
pub mod multibyte;
pub mod parser;
#[rustfmt::skip]
pub mod php_grammar;
pub mod php_recognizer;
pub mod php_scanner;
pub mod token;

pub use ast::Program;
pub use lexer::Lexer;
pub use parser::{ParseError, Parser};

use std::cell::RefCell;

thread_local! {
    /// Deprecation diagnostics emitted by the lexer/parser during the most recent
    /// pass. Drained by callers (the SAPI / compiler / phpt harness) after parse
    /// so they appear in the expected source order alongside other diagnostics.
    static LEXER_DEPRECATIONS: RefCell<Vec<(String, u32, u32)>> = const { RefCell::new(Vec::new()) };
    /// Byte ranges of code the compiler never compiles (the right operand
    /// of `&&` / `||` decided by a literal left operand): compile-time
    /// diagnostics recorded inside them are dropped.
    static DEAD_RANGES: RefCell<Vec<(u32, u32)>> = const { RefCell::new(Vec::new()) };
    /// Anonymous class declarations found in dead code: never declared, but
    /// kept for printing (assert() messages).
    static DEAD_ANON_CLASSES: RefCell<Vec<ast::Statement>> = const { RefCell::new(Vec::new()) };
}

/// Forget the dead ranges of an earlier parse (a new source is lexed).
pub fn clear_dead_ranges() {
    DEAD_RANGES.with(|d| d.borrow_mut().clear());
}

/// Mark `start..end` (byte offsets) as never compiled.
pub fn mark_dead_range(start: u32, end: u32) {
    DEAD_RANGES.with(|d| d.borrow_mut().push((start, end)));
}

/// Record an anonymous class declared only in dead code.
pub fn push_dead_anon_class(stmt: ast::Statement) {
    DEAD_ANON_CLASSES.with(|d| d.borrow_mut().push(stmt));
}

/// Take the anonymous class declarations of dead code.
pub fn take_dead_anon_classes() -> Vec<ast::Statement> {
    DEAD_ANON_CLASSES.with(|d| std::mem::take(&mut *d.borrow_mut()))
}

thread_local! {
    /// First compile error detected by the lexer (e.g. an invalid `\u{}`
    /// escape). Reported by the parser as a parse error.
    static LEXER_ERROR: RefCell<Option<(String, u32)>> = const { RefCell::new(None) };
}

/// Record a lexer-level compile error (only the first one is kept).
pub fn push_lexer_error(msg: String, line: u32) {
    LEXER_ERROR.with(|e| {
        let mut e = e.borrow_mut();
        if e.is_none() {
            *e = Some((msg, line));
        }
    });
}

/// Whether php-src reports a parser message as a ParseError (E_PARSE):
/// syntax errors and the errors its scanner raises.
pub fn is_php_parse_error(msg: &str) -> bool {
    msg.starts_with("syntax error")
        || msg.starts_with("Unclosed '")
        || msg.starts_with("Unmatched '")
        || msg == "Cannot use \"<?=\" as an identifier"
        || msg.starts_with("The (real) cast")
        || msg.starts_with("Invalid UTF-8 codepoint")
        || msg.starts_with("Invalid numeric literal")
        || msg.starts_with("Invalid body indentation level")
        || msg.starts_with("Invalid indentation - tabs and spaces")
        || msg.starts_with("Unterminated comment")
        || msg == "memory exhausted"
}

/// A scanner error php-src raises before the file is being compiled
/// (flexible heredoc indentation of a body starting with an
/// interpolation): reported in "[no active file]" on line 0.
pub fn is_no_active_file_error(msg: &str, line: u32) -> bool {
    line == 0 && msg.starts_with("Invalid body indentation level")
}

/// Take the pending lexer error, if any.
pub fn take_lexer_error() -> Option<(String, u32)> {
    LEXER_ERROR.with(|e| e.borrow_mut().take())
}

thread_local! {
    static SHORT_OPEN_TAG: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Configure the `short_open_tag` INI setting for subsequent lexer runs.
pub fn set_short_open_tag(on: bool) {
    SHORT_OPEN_TAG.with(|c| c.set(on));
}

pub fn short_open_tag() -> bool {
    SHORT_OPEN_TAG.with(|c| c.get())
}

/// Record a deprecation diagnostic discovered by the lexer/parser.
pub fn push_lexer_deprecated(msg: String, line: u32) {
    LEXER_DEPRECATIONS.with(|d| d.borrow_mut().push((msg, line, u32::MAX)));
}

/// Record a deprecation that php-src raises while compiling the code at byte
/// offset `pos` (dropped when that code is never compiled).
pub fn push_compile_deprecated_at(msg: String, line: u32, pos: u32) {
    LEXER_DEPRECATIONS.with(|d| d.borrow_mut().push((msg, line, pos)));
}

/// Drain any deprecation diagnostics collected since the last call. Should be
/// invoked after each parse.
pub fn take_lexer_deprecations() -> Vec<(String, u32)> {
    let dead = DEAD_RANGES.with(|d| std::mem::take(&mut *d.borrow_mut()));
    LEXER_DEPRECATIONS
        .with(|d| std::mem::take(&mut *d.borrow_mut()))
        .into_iter()
        .filter(|(_, _, pos)| !dead.iter().any(|&(s, e)| *pos >= s && *pos < e))
        .map(|(m, l, _)| (m, l))
        .collect()
}

thread_local! {
    /// Compile-time warnings found by the lexer (e.g. octal escape overflow).
    static LEXER_WARNINGS: RefCell<Vec<(String, u32)>> = const { RefCell::new(Vec::new()) };
}

/// Record a compile-time warning discovered by the lexer.
pub fn push_lexer_warning(msg: String, line: u32) {
    LEXER_WARNINGS.with(|d| d.borrow_mut().push((msg, line)));
}

/// The lexer's diagnostics in the order php-src raises them: the
/// scanner's own (escape warnings, non-canonical casts) in source order,
/// then those zend_compile emits for what was scanned (`${...}`
/// interpolation). Items are (is_deprecation, message, line).
pub fn order_lexer_diagnostics(
    deprecations: &[(String, u32)],
    warnings: &[(String, u32)],
) -> Vec<(bool, String, u32)> {
    let compile_time = |m: &str| m.starts_with("Using ${");
    let mut scan: Vec<(bool, String, u32)> = deprecations
        .iter()
        .filter(|(m, _)| !compile_time(m))
        .map(|(m, l)| (true, m.clone(), *l))
        .chain(warnings.iter().map(|(m, l)| (false, m.clone(), *l)))
        .collect();
    scan.sort_by_key(|(_, _, l)| *l);
    let mut compiled: Vec<(bool, String, u32)> = deprecations
        .iter()
        .filter(|(m, _)| compile_time(m))
        .map(|(m, l)| (true, m.clone(), *l))
        .collect();
    compiled.sort_by_key(|(_, _, l)| *l);
    scan.extend(compiled);
    scan
}

/// Drain lexer warnings collected since the last call.
pub fn take_lexer_warnings() -> Vec<(String, u32)> {
    LEXER_WARNINGS.with(|d| std::mem::take(&mut *d.borrow_mut()))
}
