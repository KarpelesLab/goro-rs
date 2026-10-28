//! Regex benchmark matching tools/regex_fixtures/bench.php:
//! bench <corpus.txt>. Prints "name<TAB>matches<TAB>ms" (best of 5).

use goro_regex::php::{self, GlobalMatcher};
use goro_regex::{Limits, MatchOptions};
use std::time::Instant;

const PATS: &[(&str, &str)] = &[
    ("literal", "/fox/"),
    ("digits", r"/\d+/"),
    ("word_ing", r"/\b\w+ing\b/"),
    ("caseless", "/LOREM|IPSUM/i"),
    ("email", r"/(\w+)@(\w+)\.com/"),
    ("date", r"/(\d{4})-(\d{2})-(\d{2})/"),
    ("href", r#"/<a\s+href="([^"]*)"/i"#),
    ("alternation", r"/\b(?:apple|banana|cherry)\b/"),
    ("comment_line", r"/^\s*#.*$/m"),
    ("spaces", r"/\s+/"),
    ("utf8_dot", r"/[^\x00-\x7f]+/u"),
    ("utf8_word", r"/\w+/u"),
    ("lazy_tag", "/<a.*?>/"),
    ("not_found", r"/zzz\d+qqq/"),
];

fn main() {
    let path = std::env::args().nth(1).expect("corpus path");
    let corpus = std::fs::read(path).unwrap();
    let limits = Limits {
        match_limit: 1_000_000,
        depth_limit: 100_000,
    };
    for (name, p) in PATS {
        let re = php::compile(p.as_bytes()).ok().unwrap();
        let mut best = f64::INFINITY;
        let mut n = 0;
        for _ in 0..5 {
            let t = Instant::now();
            let mut g = GlobalMatcher::new(&re, &corpus, 0, limits);
            n = 0;
            let mut sum = 0usize;
            while g.next_match().unwrap() {
                n += 1;
                // Touch every group like preg_match_all() does.
                for k in 0..g.caps.highest_set() {
                    if let Some((s, e)) = g.caps.get(k) {
                        sum = sum.wrapping_add(e - s);
                    }
                }
            }
            std::hint::black_box(sum);
            best = best.min(t.elapsed().as_secs_f64() * 1e3);
        }
        println!("{name}\t{n}\t{best:.3}");
    }
    let re = php::compile(br"/^[\w.+-]+@\w+\.[a-z]{2,}$/").ok().unwrap();
    let short: [&[u8]; 4] = [
        b"user42@example.com",
        b"not an email",
        b"a.b.c@d.org",
        b"x@y",
    ];
    let mut m = re.matcher();
    m.limits = limits;
    let mut best = f64::INFINITY;
    let mut n = 0;
    for _ in 0..5 {
        let t = Instant::now();
        n = 0;
        for k in 0..100_000usize {
            if m.exec_opts(short[k & 3], 0, MatchOptions::default())
                .unwrap()
            {
                n += 1;
            }
        }
        best = best.min(t.elapsed().as_secs_f64() * 1e3);
    }
    println!("short_calls\t{n}\t{best:.3}");
}
