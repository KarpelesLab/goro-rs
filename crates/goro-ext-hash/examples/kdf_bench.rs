//! hash_pbkdf2() and hash_hmac() timing, for comparison with PHP:
//!   cargo run --release -p goro-ext-hash --example kdf_bench
//! The PHP counterpart is tests/fixtures/kdf_bench.php.

use goro_ext_hash::{hash_hmac, hash_pbkdf2};
use std::time::Instant;

fn main() {
    for algo in ["md5", "sha1", "sha256", "sha512"] {
        let t = Instant::now();
        let out = hash_pbkdf2(algo.as_bytes(), b"password", b"salt", 100_000, 0, false).unwrap();
        let pbkdf2 = t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        let mut sink = 0u8;
        for i in 0..200_000u32 {
            sink ^= hash_hmac(algo.as_bytes(), &i.to_le_bytes(), b"key")
                .unwrap()[0];
        }
        let hm = t.elapsed().as_secs_f64() * 1000.0;
        println!(
            "{algo:8} pbkdf2 100k: {pbkdf2:8.1} ms   200k short hmacs: {hm:8.1} ms   ({}, {sink})",
            String::from_utf8_lossy(&out[..8])
        );
    }
}
