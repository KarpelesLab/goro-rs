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

/// Record a deprecation diagnostic discovered by the lexer/parser.
pub fn push_lexer_deprecated(msg: String, line: u32) {
    LEXER_DEPRECATIONS.with(|d| d.borrow_mut().push((msg, line)));
}

/// Drain any deprecation diagnostics collected since the last call. Should be
/// invoked after each parse.
pub fn take_lexer_deprecations() -> Vec<(String, u32)> {
    LEXER_DEPRECATIONS.with(|d| std::mem::take(&mut *d.borrow_mut()))
}
