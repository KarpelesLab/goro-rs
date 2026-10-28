//! 1e6 parses of typical URLs with both parsers, and their serializations.
//! Compare with `tests/fixtures/uri_bench.php` on PHP 8.5 (whose times also
//! include creating the PHP object and the interpreter loop).
//!
//!     cargo run --release -p goro-ext-uri --example uri_bench

use goro_ext_uri::rfc3986::Uri;
use goro_ext_uri::whatwg::Url;
use std::hint::black_box;
use std::time::Instant;

const URLS: &[&str] = &[
    "https://user:pass@example.com:8080/path/to/file.php?query=1&a=2#frag",
    "https://www.example.com/index.html",
    "http://localhost/",
    "https://example.org/a/b/c/d?x=1",
];

fn time(n: usize, mut f: impl FnMut()) -> f64 {
    let t = Instant::now();
    for _ in 0..n {
        f();
    }
    t.elapsed().as_secs_f64()
}

fn main() {
    let n = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(1_000_000);
    for u in URLS {
        let b = u.as_bytes();
        let r = time(n, || {
            black_box(Uri::parse(black_box(b)).unwrap());
        });
        let w = time(n, || {
            black_box(Url::parse(black_box(b), None).unwrap());
        });
        println!("{u:<72} rfc3986 {r:.3}s  whatwg {w:.3}s");
    }
    let (u, _) = Url::parse(URLS[0].as_bytes(), None).unwrap();
    let t = time(n, || {
        black_box(black_box(&u).to_ascii_string().to_vec());
    });
    println!("whatwg toAsciiString {t:.3}s");
    let r = Uri::parse(URLS[0].as_bytes()).unwrap();
    let t = time(n, || {
        black_box(black_box(&r).to_normalized_string());
    });
    println!("rfc3986 toString {t:.3}s");
    let t = time(n, || {
        black_box(black_box(&u).with_path(b"/other/path").unwrap());
    });
    println!("whatwg withPath {t:.3}s");
    let t = time(n, || {
        black_box(black_box(&r).with_path(b"/other/path").unwrap());
    });
    println!("rfc3986 withPath {t:.3}s");
}
