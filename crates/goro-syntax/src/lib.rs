//! Lexer, parser and AST for PHP 8.5 source.
//!
//! - [`lexer`]: the token stream of `token_get_all()` ([`tokenize`]).
//! - [`parser`]: [`parse`] / [`parse_eval`] build an [`ast::Program`] in an
//!   [`Arena`], or return the [`ParseError`] PHP would report.
//! - [`ast`]: the syntax tree, with spans (byte offsets and lines) on every
//!   node; [`dump`] renders it as S-expressions.
//!
//! ```
//! let arena = goro_syntax::Arena::new();
//! let prog = goro_syntax::parse(&arena, b"<?php echo 1 + 2 * 3;").unwrap();
//! assert_eq!(goro_syntax::dump::program(&prog), "(echo (+ 1 (* 2 3)))");
//! ```

pub mod ast;
pub mod dump;
pub mod export;
pub mod lexer;
pub mod parser;
pub mod token;

pub use lexer::{LexError, Lexer, LexerOptions, tokenize};
pub use parser::{Arena, ErrorKind, ParseError, ParseOptions, parse, parse_eval, parse_with};
pub use token::{T, Token};

/// `token_get_all($src, TOKEN_PARSE)`: parses the source and returns all
/// tokens, with keywords the parser used as names reported as `T_STRING`
/// (PHP manual: token_get_all, TOKEN_PARSE).
pub fn tokenize_parse(src: &[u8], opts: ParseOptions) -> Result<Vec<Token>, ParseError> {
    let arena = Arena::new();
    let prog = parse_with(&arena, src, opts)?;
    let mut toks = tokenize(src, opts.short_open_tag);
    let mut ids = prog.reserved_identifiers;
    ids.sort_unstable();
    for t in &mut toks {
        if !t.kind.is_trivia() && ids.binary_search(&t.start).is_ok() {
            t.kind = T::String;
        }
    }
    Ok(toks)
}
