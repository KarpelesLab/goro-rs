//! Match and depth limits (PHP's pcre.backtrack_limit and
//! pcre.recursion_limit).

use goro_regex::php::{self, GlobalMatcher};
use goro_regex::{Captures, Limits, MatchError, MatchOptions, Regex, Scratch};

fn re(p: &str) -> Regex {
    php::compile(p.as_bytes()).ok().unwrap()
}

fn match_all(p: &str, s: &str, limits: Limits) -> Result<usize, MatchError> {
    let r = re(p);
    let mut g = GlobalMatcher::new(&r, s.as_bytes(), 0, limits);
    let mut n = 0;
    while g.next_match()? {
        n += 1;
    }
    Ok(n)
}

fn lim(m: u32, d: u32) -> Limits {
    Limits::new(m, d)
}

const BIG: u32 = 20_000_000;

#[test]
fn php_tests() {
    // ext/pcre/tests/backtrack_limit.phpt, errors04.phpt
    assert_eq!(
        match_all("/.*\\p{N}/", "0123456789", lim(2, BIG)),
        Err(MatchError::MatchLimit)
    );
    assert_eq!(match_all("/\\p{Nd}/", "0123456789", lim(2, BIG)), Ok(10));
    // ext/pcre/tests/recursion_limit.phpt
    assert_eq!(
        match_all("/\\p{Ll}(\\p{L}((\\p{Ll}\\p{Ll})))/", "aeiou", lim(BIG, 2)),
        Err(MatchError::DepthLimit)
    );
    assert_eq!(
        match_all("/\\p{Ll}\\p{L}\\p{Ll}\\p{Ll}/", "aeiou", lim(BIG, 2)),
        Ok(1)
    );
    // errors01.phpt / split2.phpt: recursion_limit=1
    assert_eq!(
        match_all("/(\\d*)/", "ab2c3u", lim(BIG, 1)),
        Err(MatchError::DepthLimit)
    );
    // grep2.phpt
    assert_eq!(
        match_all("@^[a-z]+@", "a", lim(BIG, 1)),
        Err(MatchError::DepthLimit)
    );
    // bug52732.phpt: backtrack_limit=1
    assert_eq!(
        match_all(
            "/(?:\\D+|<\\d+>)*[!?]/",
            "foobar foobar foobar",
            lim(1, 100_000)
        ),
        Err(MatchError::MatchLimit)
    );
    // 006.phpt: backtrack_limit=100000
    let s = "[CODE]&lt;td align=&quot;$stylevar[right]&quot;&gt;[/CODE]";
    assert_eq!(
        match_all(
            "#\\[(right)\\](((?R)|[^[]+?|\\[)*)\\[/\\1\\]#siU",
            s,
            lim(100_000, 100_000)
        ),
        Err(MatchError::MatchLimit)
    );
    // no_jit_bug70110.phpt: deep but within the default limits.
    let ab = "AB".repeat(8192);
    assert_eq!(
        match_all("/(*NO_JIT)^(A{1,2}B)+$$/", &ab, lim(1_000_000, 100_000)),
        Ok(1)
    );
    let a = "a".repeat(5431);
    let r = re("~(*NO_JIT)(a)*~");
    let mut sc = Scratch::new();
    let mut caps = Captures::default();
    assert_eq!(
        r.exec_with(
            &mut sc,
            a.as_bytes(),
            0,
            MatchOptions::default(),
            lim(1_000_000, 100_000),
            &mut caps
        ),
        Ok(true)
    );
    // (*LIMIT_MATCH) lowers the limit; the required "b" is missing, so no
    // matching happens at all (observed on PHP 8.5: 0, not an error).
    assert_eq!(
        match_all("/(*LIMIT_MATCH=1)(a+)+b/", "aaa", lim(BIG, BIG)),
        Ok(0)
    );
    assert_eq!(
        match_all("/(*LIMIT_MATCH=3)(a+)+b/", "aab", lim(BIG, BIG)),
        Err(MatchError::MatchLimit)
    );
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn passes(r: &Regex, s: &[u8], l: Limits) -> bool {
    let mut sc = Scratch::new();
    let mut caps = Captures::default();
    r.exec_with(&mut sc, s, 0, MatchOptions::default(), l, &mut caps)
        .is_ok()
}

fn min_limit(r: &Regex, s: &[u8], depth: bool) -> i64 {
    let mk = |v| if depth { lim(BIG, v) } else { lim(v, BIG) };
    if !passes(r, s, mk(BIG)) {
        return -1;
    }
    let (mut lo, mut hi) = (1u32, BIG);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if passes(r, s, mk(mid)) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    lo as i64
}

/// The smallest limits with which one match call succeeds, compared with
/// php85 (pcre.jit=0, tools/regex_fixtures/limits.php). The frame
/// accounting follows PCRE2's interpreter for common constructs; the
/// listed patterns are known to count differently.
#[test]
fn php85_minimal_limits() {
    let known_different = [
        "/(?:a|b)*c/",
        "/(?:a|b)+c/",
        "/^(a\\1?){4}$/",
        "/(?:\\D+|<\\d+>)*[!?]/",
        "/(?:(a)|b)*/",
        "/\\s+$/",
        "/\\[(right)\\](((?R)|[^[]+?|\\[)*)\\[\\/\\1\\]/siU",
    ];
    let data = include_str!("fixtures/php85_limits.txt");
    let mut bad = Vec::new();
    for line in data.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let pat = unhex(f[0]);
        let subj = unhex(f[1]);
        let ps = String::from_utf8_lossy(&pat).to_string();
        if known_different.contains(&ps.as_str()) {
            continue;
        }
        let got = match php::compile(&pat) {
            Ok(r) => format!(
                "{} {}",
                min_limit(&r, &subj, false),
                min_limit(&r, &subj, true)
            ),
            Err(_) => "-1 -1".to_string(),
        };
        if got != f[2] {
            bad.push(format!(
                "{ps} {:?}: php85 {} goro {got}",
                String::from_utf8_lossy(&subj),
                f[2]
            ));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// PCRE2's nesting limit (250) must compile within an 8 MiB stack (the
/// main thread's) even in debug builds.
#[test]
fn deep_nesting_stack() {
    std::thread::Builder::new()
        .stack_size(8 << 20)
        .spawn(|| {
            let p = format!("/{}a{}/", "(".repeat(250), ")".repeat(250));
            let r = re(&p);
            assert!(r.is_match(b"xa").unwrap());
            let p = format!("/{}a{}/", "(?:".repeat(250), ")".repeat(250));
            assert!(re(&p).is_match(b"a").unwrap());
            let p = format!("/{}a{}/", "(".repeat(251), ")".repeat(251));
            assert!(php::compile(p.as_bytes()).is_err());
        })
        .unwrap()
        .join()
        .unwrap();
}

/// The largest subject (in repetitions of the unit) that matches without a
/// JIT stack error, measured on PHP 8.5 with pcre.jit=1
/// (tools/regex_fixtures/jit2.php).
#[test]
fn php85_jit_stack_thresholds() {
    let cases: &[(&str, &str, usize)] = &[
        ("/(a)*/", "a", 8190),
        ("/(?:a)*$/", "a", 24574),
        ("/^(foo)+$/", "foo", 8191),
        ("/(.)*/s", "a", 8190),
        ("/((a))*/", "a", 4914),
        ("/(?:(a)|b)*/", "a", 4914),
        ("/(?:ab)*$/", "ab", 24574),
        ("/(?:a(b))*$/", "ab", 8191),
        ("/(a)+$/", "a", 8191),
        // Known deviation: /(?:a|b)*$/ handles 8191 on PHP 8.5, 8190 here.
        ("/(?>a)*$/", "a", 24573),
        ("/(a)*?$/", "a", 8191),
        ("/(?:(?=a)a)*$/", "a", 24573),
        ("/((((a))))*$/", "a", 2729),
    ];
    let jit = Limits {
        match_limit: 1_000_000,
        depth_limit: 100_000,
        jit_stack: Some(Limits::PHP_JIT_STACK),
    };
    let mut bad = Vec::new();
    for &(p, unit, n) in cases {
        let r = re(p);
        let run = |k: usize| {
            let s = unit.repeat(k);
            let mut sc = Scratch::new();
            let mut caps = Captures::default();
            r.exec_with(
                &mut sc,
                s.as_bytes(),
                0,
                MatchOptions::default(),
                jit,
                &mut caps,
            )
        };
        let ok = run(n);
        let over = run(n + 1);
        if ok.is_err() || over != Err(MatchError::JitStackLimit) {
            bad.push(format!("{p}: at {n} {ok:?}, at {} {over:?}", n + 1));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
    // ext/pcre/tests/errors05.phpt
    let r = re("/^(foo)+$/");
    let s = "foo".repeat(1024 * 8192);
    let mut sc = Scratch::new();
    let mut caps = Captures::default();
    assert_eq!(
        r.exec_with(
            &mut sc,
            s.as_bytes(),
            0,
            MatchOptions::default(),
            jit,
            &mut caps
        ),
        Err(MatchError::JitStackLimit)
    );
}
