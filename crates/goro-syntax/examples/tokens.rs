//! Dumps the token stream of a file, one token per line:
//! `<token name or 'c'> <line> <byte length>` (character tokens show the
//! character and no line, as `token_get_all()` returns them).
//!
//! Usage: tokens FILE [--short-tags]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args.first().expect("usage: tokens FILE [--short-tags]");
    let short = args.iter().any(|a| a == "--short-tags");
    let src = std::fs::read(path).expect("read");
    let mut out = String::new();
    for t in goro_syntax::tokenize(&src, short) {
        let text = t.text(&src);
        if t.kind.is_char() {
            out.push_str(&format!("'{}' {}\n", String::from_utf8_lossy(text), text.len()));
        } else {
            out.push_str(&format!("{} {} {}\n", t.kind.php_name().unwrap_or("?"), t.line, text.len()));
        }
    }
    print!("{}", out);
}
