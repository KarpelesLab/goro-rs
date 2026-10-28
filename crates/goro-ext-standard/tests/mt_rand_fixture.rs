//! Checks the MT19937 generator against `fixtures/mt_rand.txt`, the output
//! of `fixtures/mt_rand.php` on PHP 8.5.

use goro_ext_standard::mt_rand::{Mt19937, MtMode};

fn range(label: &str) -> Option<(i64, i64)> {
    Some(match label {
        "0-9" => (0, 9),
        "1-6" => (1, 6),
        "0-999" => (0, 999),
        "neg" => (-1000, 1000),
        "0-1" => (0, 1),
        "pow2" => (0, 1023),
        "u32" => (0, 4294967295),
        "u32m" => (0, 4294967294),
        "u32p" => (0, 4294967296),
        "big" => (0, i64::MAX),
        "full" => (i64::MIN, i64::MAX),
        "neg64" => (-5000000000, 5000000000),
        "x666" => (0, 0x66666665),
        "same" => (5, 5),
        "i31" => (0, 2147483647),
        "i31p" => (0, 2147483648),
        _ => return None,
    })
}

#[test]
fn mt_rand_fixture() {
    let data = include_str!("fixtures/mt_rand.txt");
    let mut failures = Vec::new();
    let mut n = 0;
    for line in data.lines() {
        if line.starts_with("err:") {
            continue;
        }
        let (head, vals) = line.split_once(": ").unwrap();
        let h: Vec<&str> = head.split(' ').collect();
        let seed: i64 = h[0].parse().unwrap();
        let mode = MtMode::from_int(h[1].parse().unwrap());
        let mut g = Mt19937::new(seed, mode);
        let got: Vec<i64> = match h[2] {
            "raw" | "randraw" => (0..12).map(|_| g.next_int()).collect(),
            "rand20-10" => (0..12).map(|_| g.rand_range(20, 10)).collect(),
            "rand0-99" => (0..12).map(|_| g.rand_range(0, 99)).collect(),
            "xor2000" => vec![(0..2000).fold(0, |x, _| x ^ g.next_int())],
            l => {
                let (a, b) = range(l).unwrap();
                (0..12).map(|_| g.mt_rand_range(a, b).unwrap()).collect()
            }
        };
        let want: Vec<i64> = vals.split(' ').map(|v| v.parse().unwrap()).collect();
        n += 1;
        if got != want {
            failures.push(format!("{head}\n  want {want:?}\n  got  {got:?}"));
        }
    }
    assert!(n > 300);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
