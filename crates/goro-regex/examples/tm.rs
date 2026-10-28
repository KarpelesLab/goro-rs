//! tm <php-pattern> <file>: times preg_match_all-style matching (best of 5).

use goro_regex::Limits;
use goro_regex::php::{self, GlobalMatcher};
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let re = php::compile(a[1].as_bytes()).ok().unwrap();
    let s = std::fs::read(&a[2]).unwrap();
    let mut best = f64::INFINITY;
    let mut n = 0;
    for _ in 0..5 {
        let t = Instant::now();
        let mut g = GlobalMatcher::new(&re, &s, 0, Limits::new(1_000_000, 100_000));
        n = 0;
        while g.next_match().unwrap() {
            n += 1;
        }
        best = best.min(t.elapsed().as_secs_f64() * 1e3);
    }
    println!(
        "{n} matches {best:.3} ms onepass={}",
        re.prog().onepass.is_some()
    );
}
