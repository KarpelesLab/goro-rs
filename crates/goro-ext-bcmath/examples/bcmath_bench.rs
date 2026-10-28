//! Throughput of the bcmath functions; `examples/bench.php` runs the same
//! operations on PHP for comparison.
//!
//! cargo run --release -p goro-ext-bcmath --example bcmath_bench

use goro_ext_bcmath::{Number, NumberArg, bcadd, bccomp, bcdiv, bcmul, bcpow, bcsqrt, bcsub};
use std::hint::black_box;
use std::time::Instant;

fn gen_digits(seed: u64, n: usize) -> Vec<u8> {
    let mut x = seed;
    let mut s = Vec::with_capacity(n);
    for i in 0..n {
        x = (x * 1_103_515_245 + 12_345) % 2_147_483_648;
        let mut d = (x >> 16) % 10;
        if i == 0 && d == 0 {
            d = 1;
        }
        s.push(b'0' + d as u8);
    }
    s
}

/// Best of five timed runs (the machine is shared).
fn bench(label: &str, iters: u32, mut f: impl FnMut()) {
    // BC_ONLY=<substring> runs only the matching cases (for profiling).
    if let Ok(only) = std::env::var("BC_ONLY")
        && !label.contains(only.as_str())
    {
        return;
    }
    f();
    let mut best = f64::MAX;
    for _ in 0..5 {
        let t = Instant::now();
        for _ in 0..iters {
            f();
        }
        best = best.min(t.elapsed().as_nanos() as f64 / iters as f64);
    }
    println!("{label:<36} {best:>12.1} ns/op");
}

fn main() {
    let filter = std::env::args().nth(1).unwrap_or_default();
    let a = b"12345678901234567890.1234567890";
    let b = b"98765432109876543210.9876543210";
    let n = 1_000_000;
    let run = |label: &str| label.contains(filter.as_str());
    if run("small") {
        // The engine's path: a reused scratch buffer, then one allocation
        // for the PHP string (modelled by the boxed copy).
        let mut buf = Vec::new();
        type IntoFn = fn(
            &mut Vec<u8>,
            &[u8],
            &[u8],
            Option<i64>,
            u32,
        ) -> Result<(), goro_ext_bcmath::BcError>;
        let ops: [(&str, IntoFn); 4] = [
            ("bcadd", goro_ext_bcmath::bcadd_into),
            ("bcsub", goro_ext_bcmath::bcsub_into),
            ("bcmul", goro_ext_bcmath::bcmul_into),
            ("bcdiv", goro_ext_bcmath::bcdiv_into),
        ];
        let only = std::env::var("BC_ONLY").unwrap_or_default();
        for (name, f) in ops {
            if !name.contains(only.as_str()) {
                continue;
            }
            bench(&format!("{name} 20.10 digits x1e6"), n, || {
                buf.clear();
                f(&mut buf, black_box(a), black_box(b), Some(10), 0).unwrap();
                black_box(Box::<[u8]>::from(&buf[..]));
            });
        }
        bench("bcadd (Vec API) x1e6", n, || {
            black_box(bcadd(black_box(a), black_box(b), Some(10), 0).unwrap());
        });
        let _ = (bcsub, bcmul, bcdiv);
        bench("bccomp 20.10 digits x1e6", n, || {
            black_box(bccomp(black_box(a), black_box(b), Some(10), 0).unwrap());
        });
        let x = Number::parse(a).unwrap();
        let y = Number::parse(b).unwrap();
        bench("Number + 20.10 digits x1e6", n, || {
            black_box(
                Number::binary_op(
                    goro_ext_bcmath::BinOp::Add,
                    NumberArg::Number(black_box(&x)),
                    NumberArg::Number(black_box(&y)),
                )
                .unwrap(),
            );
        });
    }
    if run("large") {
        for d in [100usize, 300, 1000, 10_000, 100_000] {
            let p = gen_digits(1, d);
            let q = gen_digits(2, d);
            let h = gen_digits(3, d / 2);
            let it = if d >= 100_000 {
                3
            } else if d >= 10_000 {
                20
            } else {
                500
            };
            bench(&format!("bcmul {d}x{d}"), it, || {
                black_box(bcmul(&p, &q, None, 0).unwrap());
            });
            bench(&format!("bcdiv {d}/{} scale 0", d / 2), it, || {
                black_box(bcdiv(&p, &h, Some(0), 0).unwrap());
            });
            bench(&format!("bcdiv {d}/{d} scale {d}"), it, || {
                black_box(bcdiv(&p, &q, Some(d as i64), 0).unwrap());
            });
            bench(&format!("bcsqrt {d} scale 0"), it, || {
                black_box(bcsqrt(&p, Some(0), 0).unwrap());
            });
            bench(&format!("bcadd {d}+{d}"), it * 20, || {
                black_box(bcadd(&p, &q, None, 0).unwrap());
            });
        }
        bench("bcpow 7^50000", 3, || {
            black_box(bcpow(b"7", b"50000", None, 0).unwrap());
        });
        bench("bcpow 1.0001^10000 scale 20", 3, || {
            black_box(bcpow(b"1.0001", b"10000", Some(20), 0).unwrap());
        });
        bench("bcpow 123456789.987654321^1000", 3, || {
            black_box(bcpow(b"123456789.987654321", b"1000", Some(0), 0).unwrap());
        });
    }
    if filter == "huge" {
        // PHP 8.5 needs about 86 s for this one (an exact 7M-digit power).
        bench("bcpow 0.9999999^1000000 scale 20", 1, || {
            black_box(bcpow(b"0.9999999", b"1000000", Some(20), 0).unwrap());
        });
    }
}
