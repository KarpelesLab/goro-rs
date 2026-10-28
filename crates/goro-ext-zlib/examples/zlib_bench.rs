//! Throughput of gzcompress()/gzuncompress() on the inputs of
//! tests/fixtures/zlib_bench.php (run that on the reference PHP for
//! comparison):
//!   cargo run --release -p goro-ext-zlib --example zlib_bench

#[path = "../tests/common/mod.rs"]
mod common;

use goro_ext_zlib::{ENCODING_DEFLATE, gzcompress, gzuncompress};
use std::time::Instant;

fn main() {
    let sets = [
        ("mixed", common::mixed_data(5, 4_000_000)),
        ("text", common::text_data(4, 4_000_000)),
    ];
    for (name, data) in &sets {
        let mb = data.len() as f64 / 1e6;
        for level in [1, 6, 9] {
            let mut best = f64::INFINITY;
            let mut c = Vec::new();
            for _ in 0..5 {
                let t = Instant::now();
                c = gzcompress(data, level, ENCODING_DEFLATE).unwrap();
                best = best.min(t.elapsed().as_secs_f64());
            }
            let mut bestd = f64::INFINITY;
            let mut d = Vec::new();
            for _ in 0..10 {
                let t = Instant::now();
                d = gzuncompress(&c, 0).unwrap();
                bestd = bestd.min(t.elapsed().as_secs_f64());
            }
            assert!(d == *data, "round trip");
            println!(
                "{name:<6} level {level}  ratio {:5.3}  compress {:7.1} MB/s  uncompress {:7.1} MB/s",
                c.len() as f64 / data.len() as f64,
                mb / best,
                mb / bestd
            );
        }
    }
}
