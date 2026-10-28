//! Checks that `sort_by` makes the comparator calls PHP 8.5's `usort()`
//! makes, call for call (`fixtures/sort_trace.txt`).

use goro_ext_standard::sort::sort_by;

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[test]
fn sort_traces() {
    let data = include_str!("fixtures/sort_trace.txt");
    let mut n = 0;
    let mut failures = Vec::new();
    for line in data.lines().filter(|l| !l.starts_with('#')) {
        let (input, rest) = line.split_once(" : ").unwrap();
        let (count, hash) = rest.split_once(' ').unwrap();
        let mut v: Vec<i64> = input
            .split_whitespace()
            .map(|x| x.parse().unwrap())
            .collect();
        let mut sorted = v.clone();
        sorted.sort();
        let mut calls = Vec::new();
        sort_by(&mut v, |a, b| {
            calls.push(format!("{a},{b}"));
            a.cmp(b)
        });
        let joined = calls.join(" ");
        n += 1;
        if v != sorted
            || calls.len().to_string() != count
            || format!("{:016x}", fnv(&joined)) != hash
        {
            failures.push(format!("n={} input {input:.80}", v.len()));
        }
    }
    assert!(n > 1200, "{n}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
