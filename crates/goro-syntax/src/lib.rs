//! Lexer, parser and AST for PHP 8.5 source.
//!
//! - [`lexer`]: the token stream of `token_get_all()`.
//! - [`parser`]: [`parse`] / [`parse_eval`] build an [`ast::Program`] or
//!   return the [`ParseError`] PHP would report.
//! - [`ast`]: the syntax tree, with spans on every node.

pub mod ast;
pub mod dump;
pub mod lexer;
pub mod parser;
pub mod token;

pub use lexer::{LexError, Lexer, LexerOptions, tokenize};
pub use parser::{ErrorKind, ParseError, ParseOptions, parse, parse_eval, parse_with};
pub use token::{T, Token};
