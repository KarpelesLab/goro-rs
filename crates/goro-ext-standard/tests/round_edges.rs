//! Checks `round()` at extreme precisions against `fixtures/round_edges.txt`,
//! the output of `fixtures/round_edges.php` on PHP 8.5.

use goro_ext_standard::math::{RoundMode, round};

fn f(h: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(h, 16).unwrap())
}

#[test]
fn round_edges() {
    let data = include_str!("fixtures/round_edges.txt");
    let neg_ps: Vec<i64> = (1..=40)
        .map(|k| -k)
        .chain([-100, -300, -307, -308, -309, -310, -320, -400, -1000])
        .collect();
    let pos_ps: Vec<i64> = (10..=30)
        .chain([100, 290, 300, 308, 310, 320, 323, 324, 330, 400])
        .collect();
    let mut wide_ps: Vec<i64> = Vec::new();
    let mut seen_ps_line = false;
    let mut failures = Vec::new();
    let mut n = 0;
    for line in data.lines() {
        let (head, rest) = line.split_once(':').unwrap();
        if head == "ps" {
            wide_ps = rest.split(' ').map(|p| p.parse().unwrap()).collect();
            seen_ps_line = true;
            continue;
        }
        let h: Vec<&str> = head.split(' ').collect();
        let v = f(h[1]);
        let mode = match h.get(2) {
            Some(m) => RoundMode::from_int(m.parse().unwrap()).unwrap(),
            None => RoundMode::HalfAwayFromZero,
        };
        let ps = if seen_ps_line {
            &wide_ps
        } else if h[0] == "neg" {
            &neg_ps
        } else {
            &pos_ps
        };
        for (p, want) in ps.iter().zip(rest.split_whitespace()) {
            n += 1;
            let got = round(v, *p, mode);
            if got.to_bits() != f(want).to_bits() {
                failures.push(format!(
                    "round({v:e}, {p}, {mode:?}): want {:e} got {got:e}",
                    f(want)
                ));
            }
        }
    }
    assert!(n > 2500, "{n}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
