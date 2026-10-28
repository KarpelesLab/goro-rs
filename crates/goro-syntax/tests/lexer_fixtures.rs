//! Compares the lexer with `token_get_all()` output captured from PHP 8.5
//! (tools/gen_lexer_fixtures.php).

use goro_syntax::tokenize;

/// PHP's `stripcslashes()` (PHP manual: stripcslashes).
fn stripcslashes(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if s[i] != b'\\' || i + 1 >= s.len() {
            out.push(s[i]);
            i += 1;
            continue;
        }
        i += 1;
        let c = s[i];
        match c {
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'r' => out.push(b'\r'),
            b'a' => out.push(7),
            b'v' => out.push(11),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'x' if i + 1 < s.len() && s[i + 1].is_ascii_hexdigit() => {
                let mut v = 0u32;
                let mut n = 0;
                while n < 2 && i + 1 < s.len() && s[i + 1].is_ascii_hexdigit() {
                    i += 1;
                    v = v * 16 + (s[i] as char).to_digit(16).unwrap();
                    n += 1;
                }
                out.push(v as u8);
            }
            b'0'..=b'7' => {
                let mut v = (c - b'0') as u32;
                let mut n = 1;
                while n < 3 && i + 1 < s.len() && (b'0'..=b'7').contains(&s[i + 1]) {
                    i += 1;
                    v = v * 8 + (s[i] - b'0') as u32;
                    n += 1;
                }
                out.push(v as u8);
            }
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

#[test]
fn lexer_matches_php85() {
    compare(
        include_str!("data/lexer_cases.txt"),
        include_str!("data/lexer_expected.txt"),
        // The reference runs with short_open_tag=Off (production php.ini).
        &|src| tokenize(src, false),
    );
}

#[test]
fn token_parse_matches_php85() {
    let opts = goro_syntax::ParseOptions {
        short_open_tag: false,
        ..Default::default()
    };
    compare(
        include_str!("data/token_parse_cases.txt"),
        include_str!("data/token_parse_expected.txt"),
        &|src| goro_syntax::tokenize_parse(src, opts).expect("parses"),
    );
}

fn compare(cases: &str, expected: &str, lex: &dyn Fn(&[u8]) -> Vec<goro_syntax::Token>) {
    let cases: Vec<&str> = cases.trim_end_matches('\n').split("\n%%%%\n").collect();
    let expected: Vec<&str> = expected.trim_end_matches('\n').split("\n%%%%\n").collect();
    assert_eq!(cases.len(), expected.len());
    let mut failures = 0;
    for (case, exp) in cases.iter().zip(expected.iter()) {
        let src = if let Some(rest) = case.strip_prefix("#escaped#") {
            stripcslashes(rest.as_bytes())
        } else {
            case.as_bytes().to_vec()
        };
        let toks = lex(&src);
        let got: Vec<String> = toks
            .iter()
            .map(|t| {
                let text: String = t.text(&src).iter().map(|b| format!("{:02x}", b)).collect();
                if t.kind.is_char() {
                    format!("0 0 {}", text)
                } else {
                    format!("{} {} {}", t.kind.id(), t.line, text)
                }
            })
            .collect();
        let exp: Vec<String> = if exp.is_empty() {
            vec![]
        } else {
            exp.lines().map(String::from).collect()
        };
        if got != exp {
            failures += 1;
            eprintln!("--- case {:?}", String::from_utf8_lossy(&src));
            let n = got.len().max(exp.len());
            for i in 0..n {
                let g = got.get(i).map(String::as_str).unwrap_or("");
                let e = exp.get(i).map(String::as_str).unwrap_or("");
                let fmt = |s: &str| {
                    let mut p = s.splitn(3, ' ');
                    let id: u16 = p.next().unwrap_or("0").parse().unwrap_or(0);
                    let line = p.next().unwrap_or("");
                    let text = String::from_utf8_lossy(&unhex(p.next().unwrap_or(""))).into_owned();
                    let name = goro_syntax::T::token_name(id as i64);
                    format!(
                        "{} L{} {:?}",
                        if id == 0 {
                            "char".to_string()
                        } else {
                            name.to_string()
                        },
                        line,
                        text
                    )
                };
                let mark = if g == e { "  " } else { "!!" };
                eprintln!("{} got {:<40} exp {}", mark, fmt(g), fmt(e));
            }
        }
    }
    assert_eq!(failures, 0, "{} lexer cases differ", failures);
}
