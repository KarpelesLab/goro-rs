//! Throughput of hash() on a 1 MiB input, for comparison with PHP:
//!   cargo run --release -p goro-ext-hash --example hash_bench [algo...]
//! The PHP counterpart is tests/fixtures/hash_bench.php.

use goro_ext_hash::{HashOptions, Hasher, digest, lookup};
use std::time::Instant;

fn main() {
    let data: Vec<u8> = (0..1u32 << 20)
        .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect();
    let mut names: Vec<String> = std::env::args().skip(1).collect();
    if names.is_empty() {
        names = [
            "md5", "sha1", "sha256", "sha512", "sha3-256", "crc32b", "xxh3", "xxh128", "murmur3a",
            "murmur3f", "xxh64",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
    }
    for name in names {
        let algo = lookup(name.as_bytes()).expect("algorithm");
        let mut best_oneshot = f64::MAX;
        let mut best_stream = f64::MAX;
        let mut sink = 0u8;
        for _ in 0..5 {
            let iters = 20;
            let t = Instant::now();
            for _ in 0..iters {
                sink ^= digest(algo, &data, &HashOptions::default(), &mut Vec::new()).unwrap()[0];
            }
            best_oneshot = best_oneshot.min(t.elapsed().as_secs_f64() / iters as f64);
            let t = Instant::now();
            for _ in 0..iters {
                let mut h = Hasher::plain(algo);
                h.update(&data);
                sink ^= h.finish()[0];
            }
            best_stream = best_stream.min(t.elapsed().as_secs_f64() / iters as f64);
        }
        println!(
            "{name:10} hash(): {:8.1} MB/s   context: {:8.1} MB/s   ({sink})",
            1.048576 / best_oneshot,
            1.048576 / best_stream
        );
    }
}
