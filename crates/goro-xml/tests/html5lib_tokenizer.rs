//! html5lib-tests tokenizer runner (tokens only; PHP reports a different
//! set of tokenizer errors than the standard, see `tokenizer.rs`).
//!
//! ```text
//! HTML5LIB_TOKENIZER_DIR=/path/to/html5lib-tests/tokenizer cargo test -p goro-xml \
//!     --test html5lib_tokenizer -- --ignored --nocapture
//! ```
//!
//! Tests marked `doubleEscaped` (lone surrogates) are skipped.

mod common;

use common::Json;
use goro_xml::html5::testing::{TestToken, tokenize};

fn opt_str(j: &Json) -> Option<String> {
    match j {
        Json::Null => None,
        Json::Str(s) => Some(s.clone()),
        _ => panic!("bad"),
    }
}

fn expected(out: &Json) -> Vec<TestToken> {
    let mut v: Vec<TestToken> = Vec::new();
    for t in out.arr() {
        let a = t.arr();
        let tok = match a[0].str() {
            "DOCTYPE" => TestToken::Doctype(
                opt_str(&a[1]),
                opt_str(&a[2]),
                opt_str(&a[3]),
                a[4] == Json::Bool(true),
            ),
            "StartTag" => {
                let attrs = match &a[2] {
                    Json::Obj(o) => o
                        .iter()
                        .map(|(k, v)| (k.clone(), v.str().to_string()))
                        .collect(),
                    _ => Vec::new(),
                };
                TestToken::StartTag(
                    a[1].str().to_string(),
                    attrs,
                    a.get(3) == Some(&Json::Bool(true)),
                )
            }
            "EndTag" => TestToken::EndTag(a[1].str().to_string()),
            "Comment" => TestToken::Comment(a[1].str().to_string()),
            "Character" => {
                if let Some(TestToken::Character(s)) = v.last_mut() {
                    s.push_str(a[1].str());
                    continue;
                }
                TestToken::Character(a[1].str().to_string())
            }
            k => panic!("token kind {k}"),
        };
        v.push(tok);
    }
    v
}

#[test]
#[ignore]
fn html5lib_tokenizer() {
    let Ok(dir) = std::env::var("HTML5LIB_TOKENIZER_DIR") else {
        eprintln!("HTML5LIB_TOKENIZER_DIR not set");
        return;
    };
    let verbose = std::env::var("HTML5LIB_VERBOSE").is_ok();
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "test"))
        .collect();
    files.sort();
    let (mut pass, mut fail, mut skip) = (0, 0, 0);
    for f in files {
        let j = common::parse(&std::fs::read_to_string(&f).unwrap());
        let Some(tests) = j.get("tests") else {
            continue;
        };
        let (mut fp, mut ff) = (0, 0);
        for t in tests.arr() {
            if t.get("doubleEscaped") == Some(&Json::Bool(true)) {
                skip += 1;
                continue;
            }
            let input = t.get("input").unwrap().str();
            let exp = expected(t.get("output").unwrap());
            let last = t.get("lastStartTag").map(|l| l.str().to_string());
            let states: Vec<String> = match t.get("initialStates") {
                Some(s) => s.arr().iter().map(|x| x.str().to_string()).collect(),
                None => vec!["Data state".into()],
            };
            for st in states {
                let got = tokenize(input, &st, last.as_deref());
                if got == exp {
                    fp += 1;
                } else {
                    ff += 1;
                    if verbose {
                        eprintln!(
                            "FAIL {} [{st}] {input:?}\n  exp {exp:?}\n  got {got:?}",
                            f.display()
                        );
                    }
                }
            }
        }
        eprintln!(
            "{}: {fp} passed, {ff} failed",
            f.file_name().unwrap().to_string_lossy()
        );
        pass += fp;
        fail += ff;
    }
    eprintln!(
        "TOTAL: {pass} passed, {fail} failed, {skip} skipped (doubleEscaped); {:.2}%",
        100.0 * pass as f64 / (pass + fail).max(1) as f64
    );
}
