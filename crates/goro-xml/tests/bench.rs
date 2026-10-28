//! Parse + serialize timings on a generated document (ignored by default):
//! `cargo test --release -p goro-xml --test bench -- --ignored --nocapture`.
//! The same document is produced by tests/fixtures/bench.php for PHP.

use std::time::Instant;

use goro_xml::Arena;
use goro_xml::html::{HtmlSaveOptions, parse_html, save_html};
use goro_xml::serialize::{SaveOptions, save_xml};
use goro_xml::xml::{ParseOptions, build};

/// The benchmark document: `n` records (same text as bench.php builds).
pub fn document(n: usize) -> String {
    let mut s =
        String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<catalog xmlns:x=\"urn:x\">\n");
    for i in 0..n {
        s.push_str(&format!(
            "  <item id=\"i{i}\" x:kind=\"k{}\" price=\"{}.99\">\n    <name>Item number {i} &amp; friends</name>\n    <desc>Some descriptive text for item {i}, with &lt;markup&gt; and caf\u{e9}.</desc>\n    <tags><tag>a</tag><tag>b{}</tag><x:tag/></tags>\n    <!-- comment {i} -->\n  </item>\n",
            i % 7,
            i % 100,
            i % 13
        ));
    }
    s.push_str("</catalog>\n");
    s
}

fn html_document(n: usize) -> String {
    let mut s = String::from("<!DOCTYPE html><html><head><title>Bench</title></head><body>\n");
    for i in 0..n {
        s.push_str(&format!(
            "<div class=\"row r{}\" id=\"d{i}\"><p>Paragraph {i} with <b>bold</b> and <a href=\"/x?a={i}&amp;b=2\">a link</a>.<br>Line two &copy; 2026</p><ul><li>one<li>two</ul></div>\n",
            i % 5
        ));
    }
    s.push_str("</body></html>\n");
    s
}

/// Records in the documents (`BENCH_N`, default 20000).
fn bench_n() -> usize {
    std::env::var("BENCH_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20000)
}

#[test]
#[ignore]
fn bench_parse_serialize() {
    let doc = document(bench_n());
    let bytes = doc.as_bytes();
    let mut best_parse = f64::MAX;
    let mut best_save = f64::MAX;
    let mut out_len = 0;
    for _ in 0..5 {
        let mut arena = Arena::new();
        let t = Instant::now();
        let b = build(&mut arena, bytes, &ParseOptions::default(), None);
        let p = t.elapsed().as_secs_f64();
        assert!(b.ok);
        let t = Instant::now();
        let out = save_xml(&arena, b.doc, &SaveOptions::default());
        let s = t.elapsed().as_secs_f64();
        out_len = out.len();
        best_parse = best_parse.min(p);
        best_save = best_save.min(s);
    }
    eprintln!(
        "XML {} bytes: parse {:.1} ms, saveXML {:.1} ms (output {} bytes)",
        bytes.len(),
        best_parse * 1000.0,
        best_save * 1000.0,
        out_len
    );
    let h = html_document(bench_n());
    let mut best_parse = f64::MAX;
    let mut best_save = f64::MAX;
    for _ in 0..5 {
        let mut arena = Arena::new();
        let t = Instant::now();
        let p = parse_html(&mut arena, h.as_bytes(), 0, None, None);
        let pt = t.elapsed().as_secs_f64();
        let t = Instant::now();
        let out = save_html(&arena, p.doc, HtmlSaveOptions::default());
        let st = t.elapsed().as_secs_f64();
        out_len = out.len();
        best_parse = best_parse.min(pt);
        best_save = best_save.min(st);
    }
    eprintln!(
        "HTML {} bytes: loadHTML {:.1} ms, saveHTML {:.1} ms (output {} bytes)",
        h.len(),
        best_parse * 1000.0,
        best_save * 1000.0,
        out_len
    );
}
