//! Compares the engine with results recorded from one batched PHP 8.5 run
//! (pcre.jit=0) of preg_match_all($pattern, $subject, $m,
//! PREG_SET_ORDER | PREG_OFFSET_CAPTURE) over feature cases and over the
//! literal preg_* calls found in php-src's tests. See fixtures/README.

use goro_regex::php::{self, GlobalMatcher};
use goro_regex::{Limits, MatchError};

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// PHP's defaults: pcre.backtrack_limit=1000000, pcre.recursion_limit=100000.
pub fn php_limits() -> Limits {
    Limits {
        match_limit: 1_000_000,
        depth_limit: 100_000,
    }
}

pub fn run_match_all(pat: &[u8], subj: &[u8]) -> String {
    let re = match php::compile(pat) {
        Ok(r) => r,
        Err(e) => return format!("ERR {}", e.message()),
    };
    let mut g = GlobalMatcher::new(&re, subj, 0, php_limits());
    let mut n = 0;
    let mut body = String::new();
    loop {
        match g.next_match() {
            Ok(true) => {
                n += 1;
                body.push_str(" |");
                let caps = &g.caps;
                let hs = caps.highest_set();
                for k in 0..hs {
                    match caps.get(k) {
                        Some((s, e)) => body.push_str(&format!(" {k}={s},{e}")),
                        None => body.push_str(&format!(" {k}=-")),
                    }
                }
                if let Some(m) = caps.mark() {
                    body.push_str(&format!(" MARK={}", hex(m)));
                }
            }
            Ok(false) => break,
            Err(e) => return format!("FALSE {}", php::preg_error_code(&e)),
        }
    }
    let _ = MatchError::MatchLimit;
    format!("N {n}{body}")
}

#[test]
fn php85_match_all() {
    // Deeply nested patterns (250 levels) need more stack than a test
    // thread's default in debug builds.
    std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(check_match_all)
        .unwrap()
        .join()
        .unwrap();
}

fn check_match_all() {
    let data = include_str!("fixtures/php85_match_all.txt");
    let mut fails = Vec::new();
    let mut total = 0;
    for line in data.lines() {
        let mut parts = line.splitn(3, '\t');
        let (p, s, exp) = (
            parts.next().unwrap(),
            parts.next().unwrap(),
            parts.next().unwrap(),
        );
        let pat = unhex(p);
        let subj = unhex(s);
        total += 1;
        let got = std::panic::catch_unwind(|| run_match_all(&pat, &subj))
            .unwrap_or_else(|_| "PANIC".to_string());
        if got != exp {
            fails.push(format!(
                "pattern {:?} subject {:?}\n   expected: {}\n   got:      {}",
                String::from_utf8_lossy(&pat),
                String::from_utf8_lossy(&subj),
                exp,
                got
            ));
        }
    }
    for f in &fails {
        eprintln!("{f}");
    }
    assert!(
        fails.is_empty(),
        "{} of {} fixture cases differ",
        fails.len(),
        total
    );
}
