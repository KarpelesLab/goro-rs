//! For each line "hex(pattern) TAB hex(subject) [TAB expected]" prints the
//! smallest match and depth limits with which one match call succeeds (or
//! fails without error), like tools/regex_fixtures/limits.php does on php85.

use goro_regex::{Limits, MatchError, MatchOptions, Scratch, php};

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn ok(re: &goro_regex::Regex, s: &[u8], l: Limits) -> bool {
    let mut sc = Scratch::new();
    let mut caps = goro_regex::Captures::default();
    !matches!(
        re.exec_with(&mut sc, s, 0, MatchOptions::default(), l, &mut caps),
        Err(MatchError::MatchLimit | MatchError::DepthLimit)
    ) && re
        .exec_with(&mut sc, s, 0, MatchOptions::default(), l, &mut caps)
        .is_ok()
}

fn min_limit(re: &goro_regex::Regex, s: &[u8], depth: bool) -> i64 {
    let big = 20_000_000u32;
    let mk = |v: u32| {
        if depth {
            Limits::new(big, v)
        } else {
            Limits::new(v, big)
        }
    };
    if !ok(re, s, mk(big)) {
        return -1;
    }
    let (mut lo, mut hi) = (1u32, big);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if ok(re, s, mk(mid)) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    lo as i64
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = std::fs::read_to_string(&args[1]).unwrap();
    for line in data.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let pat = unhex(f[0]);
        let subj = unhex(f[1]);
        let r = match php::compile(&pat) {
            Ok(re) => format!(
                "{} {}",
                min_limit(&re, &subj, false),
                min_limit(&re, &subj, true)
            ),
            Err(_) => "-1 -1".to_string(),
        };
        let exp = f.get(2).copied().unwrap_or("");
        let mark = if exp.is_empty() || exp == r {
            ""
        } else {
            "   <<< php85: "
        };
        println!(
            "{:40} {:20} {}{}{}",
            String::from_utf8_lossy(&pat),
            format!("{:?}", String::from_utf8_lossy(&subj)),
            r,
            mark,
            if mark.is_empty() { "" } else { exp }
        );
    }
}
