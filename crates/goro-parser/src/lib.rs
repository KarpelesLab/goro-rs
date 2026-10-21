pub mod ast;
pub mod lexer;
pub mod parser;
pub mod token;

pub use ast::Program;
pub use lexer::Lexer;
pub use parser::{ParseError, Parser};

use std::cell::RefCell;

thread_local! {
    /// Deprecation diagnostics emitted by the lexer/parser during the most recent
    /// pass. Drained by callers (the SAPI / compiler / phpt harness) after parse
    /// so they appear in the expected source order alongside other diagnostics.
    static LEXER_DEPRECATIONS: RefCell<Vec<(String, u32)>> = const { RefCell::new(Vec::new()) };
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
    LEXER_DEPRECATIONS.with(|d| d.borrow_mut().push((msg, line)));
}

/// Drain any deprecation diagnostics collected since the last call. Should be
/// invoked after each parse.
pub fn take_lexer_deprecations() -> Vec<(String, u32)> {
    LEXER_DEPRECATIONS.with(|d| std::mem::take(&mut *d.borrow_mut()))
}
