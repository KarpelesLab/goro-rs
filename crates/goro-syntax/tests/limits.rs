//! Pathological nesting fails cleanly ("memory exhausted", as PHP reports
//! it) instead of overflowing the stack, on a default test thread.

use goro_syntax::{Arena, parse};

#[test]
fn deep_nesting_does_not_overflow() {
    // Test Zend/tests/bug64660.phpt: 17289 nested `[` exhaust PHP's parser.
    let src = format!(
        "<?php function a() {{ {}{} }}",
        "[".repeat(17289),
        "]".repeat(17289)
    );
    let arena = Arena::new();
    let e = parse(&arena, src.as_bytes()).unwrap_err();
    assert_eq!(e.message, "memory exhausted");

    for open in ["(", "f(", "$a[", "-(", "function() { return "] {
        let close = match open {
            "(" | "f(" | "-(" => ")",
            "$a[" => "]",
            _ => "; }",
        };
        let src = format!("<?php $x = {}1{};", open.repeat(50000), close.repeat(50000));
        let arena = Arena::new();
        assert!(parse(&arena, src.as_bytes()).is_err(), "{}", open);
    }
}

#[test]
fn long_operator_chains_parse() {
    // Test Zend/tests/stack_limit/stack_limit_013.phpt: 7676 nested `~`.
    let src = format!("<?php $x = {}1;", "~".repeat(7676));
    let arena = Arena::new();
    assert!(parse(&arena, src.as_bytes()).is_ok());
    // Left-associative chains do not nest.
    let src = format!("<?php $x = 1{};", " + 1".repeat(100000));
    let arena = Arena::new();
    assert!(parse(&arena, src.as_bytes()).is_ok());
}
