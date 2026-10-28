//! Lexer, parser and AST for PHP 8.5 source.

pub mod lexer;
pub mod token;

pub use lexer::{LexError, Lexer, LexerOptions, tokenize};
pub use token::{T, Token};
