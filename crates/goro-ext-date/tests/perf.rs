//! Rough timings (run with `cargo test --release -p goro-ext-date --test
//! perf -- --ignored --nocapture`).

use goro_ext_date::{TimeZone, resolve, time};
use std::time::Instant;

fn bench(name: &str, n: u32, mut f: impl FnMut(u32) -> usize) {
    let t = Instant::now();
    let mut sink = 0usize;
    for k in 0..n {
        sink = sink.wrapping_add(f(k));
    }
    let el = t.elapsed();
    println!(
        "{name}: {:.1} ns/op ({sink})",
        el.as_nanos() as f64 / n as f64
    );
}

#[test]
#[ignore]
fn timings() {
    let paris = TimeZone::from_id("Europe/Paris").unwrap();
    let ny = TimeZone::from_id("America/New_York").unwrap();
    let now = 1_793_160_715i64;
    bench("date(Y-m-d H:i:s) Paris", 1_000_000, |k| {
        time::date(b"Y-m-d H:i:s", now + k as i64, &paris).len()
    });
    bench("date(DATE_RFC2822) New York", 1_000_000, |k| {
        time::date(b"D, d M Y H:i:s O", now + k as i64, &ny).len()
    });
    bench("date(c) far future", 1_000_000, |k| {
        time::date(b"c", 4_000_000_000 + k as i64, &ny).len()
    });
    bench("offset lookup", 10_000_000, |k| {
        paris.offset_at(now + k as i64) as usize
    });
    for s in [
        "2021-06-15 13:45:56",
        "next monday",
        "+1 day",
        "@1623764756",
        "Mon, 15 Aug 2005 15:52:01 +0000",
        "first day of next month midnight",
    ] {
        bench(&format!("strtotime({s})"), 200_000, |k| {
            resolve::strtotime(s.as_bytes(), now + k as i64, &ny).unwrap_or(0) as usize
        });
    }
}
