//! Micro-benchmarks of the value model (plain timed loops, no harness).
//!
//! Run with `cargo bench -p goro-runtime`. Each line prints the time per
//! operation, the median of several runs.

use std::hint::black_box;
use std::time::Instant;

use goro_runtime::compare::loose_eq;
use goro_runtime::convert::float_to_string;
use goro_runtime::ops;
use goro_runtime::{Diagnostics, Key, KeyRef, PhpArray, PhpStr, Value};

fn bench(name: &str, ops_per_run: usize, mut f: impl FnMut()) {
    // `BENCH_FILTER=substring` runs only the matching benchmarks.
    if let Ok(filter) = std::env::var("BENCH_FILTER")
        && !name.contains(filter.as_str())
    {
        return;
    }
    let mut times = Vec::new();
    for _ in 0..7 {
        let t = Instant::now();
        f();
        times.push(t.elapsed().as_secs_f64());
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let per = times[times.len() / 2] / ops_per_run as f64 * 1e9;
    println!("{name:<44} {per:>9.2} ns/op");
}

const N: usize = 1_000_000;

fn main() {
    bench("value clone+drop (int)", N, || {
        let v = Value::Int(42);
        for _ in 0..N {
            black_box(black_box(&v).clone());
        }
    });
    bench("value clone+drop (string)", N, || {
        let v = Value::Str(PhpStr::from("hello world"));
        for _ in 0..N {
            black_box(black_box(&v).clone());
        }
    });
    bench("value clone+drop (array)", N, || {
        let v = Value::Array(PhpArray::from_values((0..10).map(Value::Int)));
        for _ in 0..N {
            black_box(black_box(&v).clone());
        }
    });
    bench("packed push (1M ints)", N, || {
        let mut a = PhpArray::new();
        for i in 0..N as i64 {
            a.push(Value::Int(i)).unwrap();
        }
        black_box(a);
    });
    let packed = PhpArray::from_values((0..N as i64).map(Value::Int));
    bench("packed get_int (1M)", N, || {
        let mut s = 0i64;
        for i in 0..N as i64 {
            if let Some(Value::Int(x)) = packed.get_int(i) {
                s += x;
            }
        }
        black_box(s);
    });
    bench("packed iterate (1M)", N, || {
        let mut s = 0i64;
        for (_, v) in packed.iter() {
            if let Value::Int(x) = v {
                s += x;
            }
        }
        black_box(s);
    });
    bench("packed overwrite in place (1M)", N, || {
        let mut a = packed.clone();
        for i in 0..N as i64 {
            a.insert(Key::Int(i), Value::Int(-i));
        }
        black_box(a);
    });
    bench("hash insert int keys (1M, sparse)", N, || {
        let mut a = PhpArray::new();
        for i in 0..N as i64 {
            a.insert(Key::Int(i.wrapping_mul(7919) ^ 0x55), Value::Int(i));
        }
        black_box(a);
    });
    let mut sparse = PhpArray::new();
    for i in 0..N as i64 {
        sparse.insert(Key::Int(i.wrapping_mul(7919) ^ 0x55), Value::Int(i));
    }
    bench("hash insert int keys (1M, sequential)", N, || {
        let mut a = PhpArray::new();
        a.insert(Key::from("x"), Value::Null);
        for i in 0..N as i64 {
            a.insert(Key::Int(i), Value::Int(i));
        }
        black_box(a);
    });
    bench("hash get_int (1M, sparse)", N, || {
        let mut s = 0i64;
        for i in 0..N as i64 {
            if let Some(Value::Int(x)) = sparse.get_int(i.wrapping_mul(7919) ^ 0x55) {
                s += x;
            }
        }
        black_box(s);
    });
    let keys: Vec<PhpStr> = (0..100_000)
        .map(|i| PhpStr::from(format!("key_{i}")))
        .collect();
    bench("hash insert string keys (100k)", keys.len(), || {
        let mut a = PhpArray::new();
        for (i, k) in keys.iter().enumerate() {
            a.insert(Key::Str(k.clone()), Value::Int(i as i64));
        }
        black_box(a);
    });
    let mut by_str = PhpArray::new();
    for (i, k) in keys.iter().enumerate() {
        by_str.insert(Key::Str(k.clone()), Value::Int(i as i64));
    }
    // Fresh strings (hash not cached yet), as from runtime concatenation.
    let probes: Vec<PhpStr> = (0..100_000)
        .map(|i| PhpStr::from(format!("key_{i}")))
        .collect();
    bench("hash get_str (100k, same strings)", keys.len(), || {
        let mut s = 0i64;
        for k in &keys {
            if let Some(Value::Int(x)) = by_str.get_str(k) {
                s += x;
            }
        }
        black_box(s);
    });
    bench("hash get_str (100k, equal copies)", probes.len(), || {
        let mut s = 0i64;
        for k in &probes {
            if let Some(Value::Int(x)) = by_str.get(KeyRef::Str(k)) {
                s += x;
            }
        }
        black_box(s);
    });
    bench("small hash ['a'=>..,'b'=>..] build+get", 100_000, || {
        let ka = PhpStr::intern(b"alpha");
        let kb = PhpStr::intern(b"beta");
        for i in 0..100_000 {
            let mut a = PhpArray::new();
            a.insert(Key::Str(ka.clone()), Value::Int(i));
            a.insert(Key::Str(kb.clone()), Value::Int(i));
            black_box(a.get_str(&kb));
        }
    });
    bench("array copy-on-write separation (1k ints)", 10_000, || {
        let a = PhpArray::from_values((0..1000).map(Value::Int));
        for _ in 0..10_000 {
            let mut b = a.clone();
            b.insert(Key::Int(0), Value::Int(1));
            black_box(b);
        }
    });
    bench("string .= 1 byte (1M)", N, || {
        let mut s = PhpStr::empty();
        for _ in 0..N {
            s.push(b'x');
        }
        black_box(s);
    });
    let mut ctx = Diagnostics::new();
    bench("baseline: inline int add on Value", N, || {
        let mut acc = Value::Int(0);
        for _ in 0..N {
            acc = match (black_box(&acc), black_box(&Value::Int(1))) {
                (Value::Int(x), Value::Int(y)) => Value::Int(x + y),
                _ => Value::Null,
            };
        }
        black_box(acc);
    });
    bench("ops::add int+int", N, || {
        let mut acc = Value::Int(0);
        let one = Value::Int(1);
        for _ in 0..N {
            acc = ops::add(&acc, &one, &mut ctx).unwrap();
        }
        black_box(acc);
    });
    bench("ops::add float+int", N, || {
        let mut acc = Value::Float(0.5);
        let one = Value::Int(1);
        for _ in 0..N {
            acc = ops::add(&acc, &one, &mut ctx).unwrap();
        }
        black_box(acc);
    });
    bench("ops::add numeric string + int", N, || {
        let s = Value::from("12345");
        let one = Value::Int(1);
        for _ in 0..N {
            black_box(ops::add(&s, &one, &mut ctx).unwrap());
        }
    });
    bench("loose_eq string == string (numeric)", N, || {
        let a = Value::from("1e3");
        let b = Value::from("1000");
        for _ in 0..N {
            black_box(loose_eq(&a, &b, &mut ctx).unwrap());
        }
    });
    bench("loose_eq string == string (non-numeric)", N, || {
        let a = Value::from("hello");
        let b = Value::from("world");
        for _ in 0..N {
            black_box(loose_eq(&a, &b, &mut ctx).unwrap());
        }
    });
    bench("ops::concat str . int", N, || {
        let a = Value::from("item");
        let b = Value::Int(12345);
        for _ in 0..N {
            black_box(ops::concat(&a, &b, &mut ctx).unwrap());
        }
    });
    bench("float to string precision 14 (0.1)", N, || {
        for _ in 0..N {
            black_box(float_to_string(black_box(0.1), 14));
        }
    });
    bench("float to string precision -1 (1/3)", N, || {
        for _ in 0..N {
            black_box(float_to_string(black_box(1.0 / 3.0), -1));
        }
    });
    bench("int to string", N, || {
        let mut acc = 0usize;
        for i in 0..N as i64 {
            acc += goro_runtime::convert::int_to_string(i * 7).len();
        }
        black_box(acc);
    });
}
