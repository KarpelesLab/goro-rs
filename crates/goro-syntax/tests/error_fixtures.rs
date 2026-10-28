//! Compares parse errors with PHP 8.5's, captured by
//! tools/gen_error_fixtures.php (token_get_all() with TOKEN_PARSE).

use goro_syntax::parse;

/// PHP's `stripcslashes()` for the escapes the case file uses.
fn unescape(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            i += 1;
            out.push(match b[i] {
                b'n' => b'\n',
                b't' => b'\t',
                b'r' => b'\r',
                c => c,
            });
        } else {
            out.push(b[i]);
        }
        i += 1;
    }
    out
}

#[test]
fn parse_errors_match_php85() {
    let cases = include_str!("data/error_cases.txt");
    let expected = include_str!("data/error_expected.txt");
    let mut failures = Vec::new();
    for (case, exp) in cases.lines().zip(expected.lines()) {
        let mut src = b"<?php ".to_vec();
        src.extend(unescape(case));
        let got = match parse(&src) {
            Ok(_) => "OK".to_string(),
            Err(e) => format!(
                "{}\t{}",
                e.line,
                e.message.replace('\\', "\\\\").replace('\n', "\\n")
            ),
        };
        if got != exp {
            failures.push(format!("{}\n    got: {}\n    exp: {}", case, got, exp));
        }
    }
    for f in &failures {
        eprintln!("{}", f);
    }
    assert!(
        failures.is_empty(),
        "{} of {} error cases differ",
        failures.len(),
        cases.lines().count()
    );
}
