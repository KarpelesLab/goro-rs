//! Throughput of the gmp functions; `examples/bench.php` runs the same
//! operations on PHP for comparison (operands from the same seeded
//! `gmp_random_bits()` stream).
//!
//! cargo run --release -p goro-ext-gmp --example gmp_bench [filter]

use std::hint::black_box;
use std::time::Instant;

use goro_ext_gmp::object::binary_op;
use goro_ext_gmp::*;
use goro_runtime::Diagnostics;
use goro_runtime::ops::BinOp;

fn val(seed: u64, bits: u64) -> Gmp {
    random::seed(&Int::from_u64(seed));
    Gmp(random::bits(bits))
}

/// Best of five timed runs (the machine is shared).
fn bench(filter: &str, label: &str, iters: u64, mut f: impl FnMut()) {
    if !label.contains(filter) {
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
    println!("{label:<36} {best:>14.1} ns/op");
}

fn main() {
    let filter = std::env::args().nth(1).unwrap_or_default();
    let f = filter.as_str();
    let mut d = Diagnostics::default();
    let mut env = Env::new(&mut d, false);
    let (a, b, c) = (val(1, 63), val(2, 62), val(3, 40));
    let m = gmp_nextprime(&mut env, Num::Gmp(&val(4, 61))).unwrap();
    let (na, nb, nc, nm) = (Num::Gmp(&a), Num::Gmp(&b), Num::Gmp(&c), Num::Gmp(&m));
    let n = 100_000;
    bench(f, "small: empty closure", n, || {
        black_box(());
    });
    bench(f, "small: gmp_add", n, || {
        black_box(gmp_add(&mut env, na, nb).unwrap());
    });
    bench(f, "small: gmp_mul", n, || {
        black_box(gmp_mul(&mut env, na, nb).unwrap());
    });
    bench(f, "small: gmp_div_qr", n, || {
        black_box(gmp_div_qr(&mut env, na, nc, 0).unwrap());
    });
    bench(f, "small: $a * $b + $c", n, || {
        let ab = binary_op(env.ctx, BinOp::Mul, na, nb).unwrap().unwrap();
        black_box(binary_op(env.ctx, BinOp::Add, Num::Gmp(&ab), nc));
    });
    bench(f, "small: gmp_cmp", n, || {
        black_box(gmp_cmp(&mut env, na, nb).unwrap());
    });
    bench(f, "small: gmp_strval", n, || {
        black_box(gmp_strval(&mut env, na, 10).unwrap());
    });
    bench(f, "small: gmp_init string", n, || {
        black_box(gmp_init(&mut env, Num::Str(b"12345678901234567890"), 0).unwrap());
    });
    bench(f, "small: gmp_intval", n, || {
        black_box(gmp_intval(&mut env, nb).unwrap());
    });
    bench(f, "small: gmp_powm 61-bit", n / 10, || {
        black_box(gmp_powm(&mut env, na, nb, nm).unwrap());
    });
    bench(f, "small: gmp_gcd", n, || {
        black_box(gmp_gcd(&mut env, na, nb).unwrap());
    });
    bench(f, "small: gmp_prob_prime 61-bit", n / 10, || {
        black_box(gmp_prob_prime(&mut env, nm, 10).unwrap());
    });
    bench(f, "small: gmp_nextprime 63-bit", n / 100, || {
        black_box(gmp_nextprime(&mut env, na).unwrap());
    });

    for bits in [1000u64, 10_000, 100_000, 1_000_000] {
        let (x, y, z) = (val(10, bits), val(11, bits), val(12, 2 * bits));
        let s = x.to_string();
        let (nx, ny, nz) = (Num::Gmp(&x), Num::Gmp(&y), Num::Gmp(&z));
        let it = (2_000_000 / bits).max(1);
        bench(f, &format!("mul {bits}x{bits}"), it, || {
            black_box(gmp_mul(&mut env, nx, ny).unwrap());
        });
        bench(f, &format!("div_q {}/{bits}", 2 * bits), it, || {
            black_box(gmp_div_q(&mut env, nz, ny, 0).unwrap());
        });
        bench(f, &format!("strval {bits} bits"), (it / 10).max(1), || {
            black_box(gmp_strval(&mut env, nx, 10).unwrap());
        });
        bench(
            f,
            &format!("init decimal {bits} bits"),
            (it / 10).max(1),
            || {
                black_box(gmp_init(&mut env, Num::Str(s.as_bytes()), 0).unwrap());
            },
        );
        bench(
            f,
            &format!("sqrt {} bits", 2 * bits),
            (it / 4).max(1),
            || {
                black_box(gmp_sqrt(&mut env, nz).unwrap());
            },
        );
        if bits <= 100_000 {
            bench(f, &format!("gcd {bits}x{bits}"), (it / 10).max(1), || {
                black_box(gmp_gcd(&mut env, nx, ny).unwrap());
            });
        }
    }
    for bits in [1024u64, 2048, 4096, 8192] {
        let (x, e) = (val(20, bits), val(21, bits));
        let mut md = val(22, bits);
        gmp_setbit(&mut md, 0, true).unwrap();
        let it = (200_000 / (bits * bits / 1024)).max(1);
        bench(f, &format!("powm {bits}"), it, || {
            black_box(gmp_powm(&mut env, Num::Gmp(&x), Num::Gmp(&e), Num::Gmp(&md)).unwrap());
        });
    }
}
