//! Timing of the HTML5 parser, serializer, XPath and CSS on a large
//! document (compare with `Dom\HTMLDocument::createFromString` timed with
//! hrtime on PHP 8.5):
//!
//! ```text
//! HTML5_BENCH_FILE=/path/bench.html cargo test --release -p goro-xml \
//!     --test html5_bench -- --ignored --nocapture
//! ```

use std::time::Instant;

use goro_xml::html5::{self, ParseOptions};
use goro_xml::tree::Arena;
use goro_xml::{css, xpath};

#[test]
#[ignore]
fn bench() {
    let Ok(path) = std::env::var("HTML5_BENCH_FILE") else {
        eprintln!("HTML5_BENCH_FILE not set");
        return;
    };
    let input = std::fs::read(path).unwrap();
    let mut best = f64::MAX;
    for _ in 0..5 {
        let mut a = Arena::new();
        let t = Instant::now();
        let r = html5::parse_document(&mut a, &input, &ParseOptions::default());
        best = best.min(t.elapsed().as_secs_f64() * 1e3);
        std::hint::black_box(r.doc);
    }
    eprintln!("bytes={} parse best_ms={best:.1}", input.len());
    let text = String::from_utf8(input.clone()).unwrap();
    let t = Instant::now();
    let toks = html5::testing::tokenize(&text, "Data state", None);
    eprintln!(
        "tokenize_ms={:.1} tokens={}",
        t.elapsed().as_secs_f64() * 1e3,
        toks.len()
    );
    let mut a = Arena::new();
    let r = html5::parse_document(&mut a, &input, &ParseOptions::default());
    let t = Instant::now();
    let s = html5::serialize_node(&a, r.doc);
    eprintln!(
        "serialize_ms={:.1} len={}",
        t.elapsed().as_secs_f64() * 1e3,
        s.len()
    );
    let e = xpath::compile("//*[local-name()=\"a\"]").unwrap();
    let t = Instant::now();
    let v = xpath::evaluate(&mut a, &e, r.doc, &mut xpath::Context::new()).unwrap();
    let n = match v {
        xpath::Value::NodeSet(v) => v.len(),
        _ => 0,
    };
    eprintln!("xpath_ms={:.1} n={n}", t.elapsed().as_secs_f64() * 1e3);
    let sel = css::parse("div.c3 > p a").unwrap();
    let t = Instant::now();
    let n = css::query_selector_all(&a, r.doc, &sel).len();
    eprintln!("css_ms={:.1} n={n}", t.elapsed().as_secs_f64() * 1e3);
}
