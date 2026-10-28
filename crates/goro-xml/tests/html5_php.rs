//! Comparison with PHP 8.5's `Dom\HTMLDocument` on a corpus.
//!
//! `tests/fixtures/html5_php/` holds a corpus of inputs (`inputs.jsonl`),
//! the driver (`driver.php`) and PHP 8.5's output for it (`php85.jsonl`:
//! the warning messages and `saveHtml()` of each input). The regular test
//! checks the committed fixture; the ignored one compares a larger corpus
//! given by `HTML5_CORPUS_IN`/`HTML5_CORPUS_PHP` and prints statistics.

#[path = "h5common/mod.rs"]
mod common;

use std::collections::BTreeMap;

use goro_xml::html5::{self, ParseOptions};
use goro_xml::tree::Arena;

fn ours(input: &str) -> (Vec<String>, String) {
    let mut arena = Arena::new();
    let r = html5::parse_document(
        &mut arena,
        input.as_bytes(),
        &ParseOptions {
            collect_errors: true,
            ..Default::default()
        },
    );
    let errs = r.errors.iter().map(|e| e.message("Entity")).collect();
    (errs, html5::serialize_node(&arena, r.doc))
}

fn code_of(msg: &str) -> String {
    // "tree error CODE in Entity, line: ..."
    let mut it = msg.split(' ');
    let kind = it.next().unwrap_or("");
    it.next();
    format!("{kind} {}", it.next().unwrap_or(""))
}

fn compare(inputs: &str, php: &str, verbose: bool) -> (usize, usize, usize) {
    let mut html_bad = 0;
    let mut err_bad = 0;
    let mut n = 0;
    let mut missing: BTreeMap<String, usize> = BTreeMap::new();
    let mut extra: BTreeMap<String, usize> = BTreeMap::new();
    for (inl, outl) in inputs.lines().zip(php.lines()) {
        n += 1;
        let input = common::parse(inl);
        let input = input.str();
        let exp = common::parse(outl);
        let exp_errs: Vec<String> = exp
            .get("e")
            .unwrap()
            .arr()
            .iter()
            .map(|e| e.str().to_string())
            .collect();
        let exp_html = exp.get("h").unwrap().str();
        let (errs, html) = ours(input);
        if html != exp_html {
            html_bad += 1;
            if verbose {
                eprintln!("HTML MISMATCH {input:?}\n  php: {exp_html:?}\n  us:  {html:?}");
            }
        }
        if errs != exp_errs {
            err_bad += 1;
            let mut e2 = exp_errs.clone();
            for e in &errs {
                if let Some(i) = e2.iter().position(|x| x == e) {
                    e2.remove(i);
                } else {
                    *extra.entry(code_of(e)).or_default() += 1;
                }
            }
            for e in &e2 {
                *missing.entry(code_of(e)).or_default() += 1;
            }
            if verbose {
                eprintln!("ERR MISMATCH {input:?}\n  php: {exp_errs:?}\n  us:  {errs:?}");
            }
        }
    }
    eprintln!("cases: {n}, html mismatches: {html_bad}, error-list mismatches: {err_bad}");
    eprintln!("missing (php reports, we don't): {missing:?}");
    eprintln!("extra (we report, php doesn't): {extra:?}");
    (n, html_bad, err_bad)
}

#[test]
fn php85_fixture() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/html5_php");
    let inputs = std::fs::read_to_string(format!("{dir}/inputs.jsonl")).unwrap();
    let php = std::fs::read_to_string(format!("{dir}/php85.jsonl")).unwrap();
    let (n, h, e) = compare(&inputs, &php, true);
    assert!(n > 0);
    assert_eq!((h, e), (0, 0));
}

#[test]
#[ignore]
fn php85_corpus() {
    let (Ok(i), Ok(p)) = (
        std::env::var("HTML5_CORPUS_IN"),
        std::env::var("HTML5_CORPUS_PHP"),
    ) else {
        eprintln!("HTML5_CORPUS_IN / HTML5_CORPUS_PHP not set");
        return;
    };
    let inputs = std::fs::read_to_string(i).unwrap();
    let php = std::fs::read_to_string(p).unwrap();
    compare(&inputs, &php, std::env::var("HTML5_VERBOSE").is_ok());
}
