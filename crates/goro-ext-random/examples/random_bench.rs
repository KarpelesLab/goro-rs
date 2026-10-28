//! Library-level timing of the hot random functions, for comparison with
//! PHP (tests/fixtures/random_bench.php runs the same loops):
//!   cargo run --release -p goro-ext-random --example random_bench
//! Each line is the best of 5 runs, in nanoseconds per call (per byte for
//! getBytes and str_shuffle).

use goro_ext_random::legacy::{LegacyRandom, random_int};
use goro_ext_random::randomizer as rz;
use goro_ext_random::{
    IntervalBoundary, Mt19937, MtMode, NativeEngine, PcgOneseq128XslRr64, Secure,
    Xoshiro256StarStar,
};
use std::hint::black_box;
use std::time::Instant;

fn best(label: &str, n: u64, mut f: impl FnMut() -> u64) {
    let mut best = f64::MAX;
    let mut sink = 0u64;
    for _ in 0..5 {
        let t = Instant::now();
        sink = sink.wrapping_add(f());
        best = best.min(t.elapsed().as_secs_f64());
    }
    black_box(sink);
    println!("{label:<36} {:8.2} ns", best * 1e9 / n as f64);
}

fn main() {
    const N: u64 = 10_000_000;
    let mut l = LegacyRandom::new();
    l.mt_srand(Some(42), 0);
    best("mt_rand()", N, || {
        (0..N).fold(0, |a, _| a ^ l.mt_rand() as u64)
    });
    best("mt_rand(1, 100)", N, || {
        (0..N).fold(0, |a, _| a ^ l.mt_rand_range(1, 100).unwrap() as u64)
    });
    best("mt_rand(0, PHP_INT_MAX)", N, || {
        (0..N).fold(0, |a, _| a ^ l.mt_rand_range(0, i64::MAX).unwrap() as u64)
    });

    let engines: Vec<(&str, NativeEngine)> = vec![
        (
            "Mt19937",
            NativeEngine::Mt19937(Box::new(Mt19937::new(42, MtMode::Mt19937))),
        ),
        ("Pcg", NativeEngine::Pcg(PcgOneseq128XslRr64::from_int(42))),
        (
            "Xoshiro",
            NativeEngine::Xoshiro(Xoshiro256StarStar::from_int(42)),
        ),
    ];
    for (name, mut e) in engines {
        best(&format!("{name} getInt(1, 100)"), N, || {
            (0..N).fold(0, |a, _| a ^ rz::get_int(&mut e, 1, 100).unwrap() as u64)
        });
        best(&format!("{name} nextInt()"), N, || {
            (0..N).fold(0, |a, _| a ^ rz::next_int(&mut e).unwrap() as u64)
        });
        best(&format!("{name} nextFloat()"), N, || {
            (0..N).fold(0, |a, _| a ^ rz::next_float(&mut e).unwrap().to_bits())
        });
        best(&format!("{name} getFloat(0, 1)"), N, || {
            (0..N).fold(0, |a, _| {
                a ^ rz::get_float(&mut e, 0.0, 1.0, IntervalBoundary::ClosedOpen)
                    .unwrap()
                    .to_bits()
            })
        });
        let len = 100 << 20;
        best(&format!("{name} getBytes(100 MiB) /byte"), len, || {
            rz::get_bytes(&mut e, len as i64).unwrap()[12345] as u64
        });
    }
    let mut s = Secure;
    best("Secure getBytes(100 MiB) /byte", 100 << 20, || {
        rz::get_bytes(&mut s, 100 << 20).unwrap()[12345] as u64
    });
    best("random_int(1, 100)", N / 10, || {
        (0..N / 10).fold(0, |a, _| a ^ random_int(1, 100).unwrap() as u64)
    });
    let mut bytes: Vec<u8> = (0..10_000_000u32).map(|i| i as u8).collect();
    best("str_shuffle(10 MB) /byte", bytes.len() as u64, || {
        l.str_shuffle(&mut bytes);
        bytes[7] as u64
    });
    let mut arr: Vec<u64> = (0..1_000_000).collect();
    best("shuffle(1e6 ints) /element", arr.len() as u64, || {
        l.shuffle(&mut arr);
        arr[7]
    });
}
