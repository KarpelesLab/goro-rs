//! Literal values compared with PHP 8.5 (tools/gen_literal_fixtures.php):
//! integers, floats (bit-exact), strings after escape decoding and heredoc
//! indentation removal.

use goro_syntax::ast::{ExprKind, StmtKind};
use goro_syntax::{Arena, parse};

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

fn hex(b: &[u8]) -> String {
    b.iter().map(|c| format!("{:02x}", c)).collect()
}

#[test]
fn literals_match_php85() {
    let cases = include_str!("data/literal_cases.txt");
    let expected = include_str!("data/literal_expected.txt");
    let mut fails = Vec::new();
    for (case, exp) in cases.lines().zip(expected.lines()) {
        let mut src = b"<?php return ".to_vec();
        src.extend(unescape(case));
        src.extend_from_slice(b"\n;");
        let arena = Arena::new();
        let got = match parse(&arena, &src) {
            Err(e) => format!("error {}", e.message),
            Ok(p) => match &p.stmts[0].kind {
                StmtKind::Return(Some(e)) => match &e.kind {
                    ExprKind::Int(i) => format!("int {}", i),
                    ExprKind::Float(f) => format!("float {:016x}", f.to_bits()),
                    ExprKind::String(s, _) => format!("string {}", hex(s)),
                    // `-0` and similar.
                    ExprKind::Unary { expr, .. } => match expr.kind {
                        ExprKind::Int(i) => format!("int {}", -i),
                        _ => "unsupported".into(),
                    },
                    k => format!("unsupported {:?}", k),
                },
                _ => "unsupported".into(),
            },
        };
        let exp = exp.trim_end();
        if got.trim_end() != exp {
            fails.push(format!("{}\n    got: {}\n    exp: {}", case, got, exp));
        }
    }
    for f in &fails {
        eprintln!("{}", f);
    }
    assert!(fails.is_empty(), "{} literal cases differ", fails.len());
}
