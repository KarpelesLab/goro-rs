//! Runs a fixture file (see tests/php_fixtures.rs), printing differences.
//! Usage: fixture_run <file> [-v]

use goro_regex::Limits;
use goro_regex::php::{self, GlobalMatcher};

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn run_match_all(pat: &[u8], subj: &[u8]) -> String {
    let re = match php::compile(pat) {
        Ok(r) => r,
        Err(e) => return format!("ERR {}", e.message()),
    };
    let mut g = GlobalMatcher::new(
        &re,
        subj,
        0,
        Limits {
            match_limit: 1_000_000,
            depth_limit: 100_000,
        },
    );
    let mut n = 0;
    let mut body = String::new();
    loop {
        match g.next_match() {
            Ok(true) => {
                n += 1;
                body.push_str(" |");
                let caps = &g.caps;
                for k in 0..caps.highest_set() {
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
    format!("N {n}{body}")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = std::fs::read_to_string(&args[1]).unwrap();
    let verbose = args.iter().any(|a| a == "-v");
    let (mut total, mut bad) = (0, 0);
    for (ln, line) in data.lines().enumerate() {
        let mut parts = line.splitn(3, '\t');
        let (p, s, exp) = (
            parts.next().unwrap(),
            parts.next().unwrap(),
            parts.next().unwrap(),
        );
        let pat = unhex(p);
        let subj = unhex(s);
        if verbose {
            eprintln!("{ln}: {:?}", String::from_utf8_lossy(&pat));
        }
        total += 1;
        let got = std::panic::catch_unwind(|| run_match_all(&pat, &subj))
            .unwrap_or_else(|_| "PANIC".into());
        if got != exp {
            bad += 1;
            println!(
                "#{ln} pattern {:?} subject {:?}\n   expected: {}\n   got:      {}",
                String::from_utf8_lossy(&pat),
                String::from_utf8_lossy(&subj),
                exp,
                got
            );
        }
    }
    println!("{bad} of {total} differ");
}
