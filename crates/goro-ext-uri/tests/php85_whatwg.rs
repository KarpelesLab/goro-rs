//! `Uri\WhatWg\Url` against fixtures recorded from PHP 8.5
//! (`tests/fixtures/whatwg_driver.php`): every input of the WPT
//! urltestdata.json, toascii.json and IdnaTestV2.json files (parsed with
//! their base), with every getter, both serializations and the full
//! validation-error list; and the setters (`whatwg_setters_driver.php`).

mod common;
use common::*;
use goro_ext_uri::whatwg::{Url, UrlError, ValidationError};

pub fn wurl(u: &Url) -> String {
    [
        q(Some(u.scheme())),
        q(u.username()),
        q(u.password()),
        q(u.ascii_host()),
        q(u.unicode_host().as_deref()),
        qi(u.port().map(i64::from)),
        q(Some(u.path())),
        q(u.query()),
        q(u.fragment()),
        q(Some(u.to_ascii_string())),
        q(Some(&u.to_unicode_string())),
    ]
    .join("|")
}

pub fn errs(list: &[ValidationError]) -> String {
    let p: Vec<String> = list
        .iter()
        .map(|e| {
            format!(
                "{},{},{}",
                e.kind.name(),
                if e.failure() { 1 } else { 0 },
                q(Some(&e.context))
            )
        })
        .collect();
    format!("[{}]", p.join(";"))
}

pub fn exc(e: &UrlError) -> String {
    format!(
        "EXC Uri\\WhatWg\\InvalidUrlException {} {}",
        q(Some(e.message().as_bytes())),
        errs(&e.errors)
    )
}

/// Inputs whose IPv4 error contexts PHP 8.5 reads from memory it has just
/// freed and reused for the path: what it shows depends on the allocator's
/// history (the same `http://4294967295` gives the part's text when parsed
/// first in a process, and "/" after a base URL), so it is not reproduced.
/// Everything else about these inputs matches.
const ALLOCATOR_DEPENDENT: &[&str] = &[
    "http://%30%78%63%30%2e%30%32%35%30.01",
    "http://%30%78%63%30%2e%30%32%35%30.01%2e",
    "http://\u{ff10}\u{ff38}\u{ff43}\u{ff10}\u{ff0e}\u{ff10}\u{ff12}\u{ff15}\u{ff10}\u{ff0e}\u{ff10}\u{ff11}",
    "http://256",
    "http://999999999.",
    "http://4294967295",
];

/// Drops the contexts of IPv4 errors (for [`ALLOCATOR_DEPENDENT`]).
fn without_ipv4_contexts(s: &str) -> String {
    let mut out = String::new();
    for (i, part) in s.split("Ipv4").enumerate() {
        if i == 0 {
            out.push_str(part);
            continue;
        }
        out.push_str("Ipv4");
        // part: "<Name>,<f>,\"...\"<rest>"
        let mut commas = 0;
        let mut j = 0;
        let b = part.as_bytes();
        while j < b.len() && commas < 2 {
            if b[j] == b',' {
                commas += 1;
            }
            j += 1;
        }
        out.push_str(&part[..j]);
        // skip the quoted context
        let rest = &part[j..];
        if let Some(stripped) = rest.strip_prefix('"') {
            let end = stripped.find('"').unwrap();
            out.push_str("\"?\"");
            out.push_str(&stripped[end + 1..]);
        } else {
            out.push_str(rest);
        }
    }
    out
}

#[test]
fn whatwg_php85_fixture() {
    run_parse("whatwg_php85.txt");
}

/// Seeded random inputs (`tests/fixtures/gen_fuzz.py`).
#[test]
fn whatwg_php85_fuzz_fixture() {
    run_parse("whatwg_fuzz_php85.txt");
}

/// `Url::parse` takes a one-pass route for common URLs; it must give the
/// full parser's result on every fixture input and on combinations of
/// typical parts.
#[test]
fn fast_path_matches_full_parser() {
    fn outcome(r: Result<(Url, Vec<ValidationError>), UrlError>) -> String {
        match r {
            Ok((u, e)) => format!("{}\t{}", wurl(&u), errs(&e)),
            Err(e) => format!("NULL\t{}\t{}", errs(&e.errors), exc(&e)),
        }
    }
    let mut inputs: Vec<Vec<u8>> = Vec::new();
    for file in ["whatwg_php85.txt", "whatwg_fuzz_php85.txt"] {
        for f in fixture(file) {
            inputs.push(unhex(&f[1]));
        }
    }
    let schemes = ["http", "HTTPS", "ws", "wss", "ftp", "file", "foo", "Http"];
    let users = [
        "", "@", "u@", "u:p@", ":p@", "u:@", "u:p:q@", "a@b@", "u%20@", "u;x@",
    ];
    let hosts = [
        "h",
        "Example.COM",
        "xn--nxasmq6b",
        "1.2.3.4",
        "0x1",
        "a.1",
        "h.",
        "",
        "a b",
        "é.x",
    ];
    let ports = [
        "", ":", ":80", ":443", ":21", ":0080", ":65535", ":65536", ":8a", "::1",
    ];
    let paths = [
        "", "/", "/a/b", "//x", "/./x", "/a/..", "/a/.", "/%41", "/a\\b", "/{x}", "/a`", "/.a/..b",
    ];
    let tails = [
        "", "?", "?q=1&'", "#", "#f'g", "?a#b", "?a#b#c", "?a b", "# ", "?%zz",
    ];
    for s in schemes {
        for (i, u) in users.iter().enumerate() {
            for (j, h) in hosts.iter().enumerate() {
                let port = ports[(i + j) % ports.len()];
                for (k, p) in paths.iter().enumerate() {
                    let tail = tails[(i + j + k) % tails.len()];
                    inputs.push(format!("{s}://{u}{h}{port}{p}{tail}").into_bytes());
                }
            }
        }
    }
    let mut bad = 0;
    for input in &inputs {
        let fast = outcome(Url::parse(input, None));
        let full = outcome(Url::parse_full(input, None));
        if fast != full {
            bad += 1;
            if bad <= 10 {
                eprintln!("{}\n  fast: {fast}\n  full: {full}", show(input));
            }
        }
    }
    assert_eq!(bad, 0, "{bad} of {} inputs differ", inputs.len());
}

fn run_parse(file: &'static str) {
    let mut rep = Report::new(file);
    for f in fixture(file) {
        let line = f[..3].join("\t");
        let input = unhex(&f[1]);
        let base = if f[2] == "-" {
            None
        } else {
            match Url::parse(&unhex(&f[2]), None) {
                Ok((b, _)) => Some(b),
                Err(_) => {
                    rep.check(|| line.clone(), "BASEFAIL", &f[3..].join("\t"));
                    continue;
                }
            }
        };
        let got = match Url::parse(&input, base.as_ref()) {
            Ok((u, e)) => format!("{}\t{}", wurl(&u), errs(&e)),
            Err(e) => format!("NULL\t{}\t{}", errs(&e.errors), exc(&e)),
        };
        let want = f[3..].join("\t");
        if ALLOCATOR_DEPENDENT.contains(&std::str::from_utf8(&input).unwrap_or("")) {
            rep.check(
                || format!("{line}  {}", show(&input)),
                &without_ipv4_contexts(&got),
                &without_ipv4_contexts(&want),
            );
            continue;
        }
        rep.check(|| format!("{line}  {}", show(&input)), &got, &want);
    }
    rep.finish();
}

/// Replaces the contexts of `want`'s errors by "?" where `got` has "?"
/// (the inherited errors, see the setters test).
fn blank_inherited(want: &str, got: &str) -> String {
    fn split_errs(s: &str) -> Option<(&str, Vec<String>)> {
        let start = s.find(" [")?;
        let list = &s[start + 2..s.len() - 1];
        // Errors are "Type,f,\"ctx\"" joined by ';' (contexts are escaped,
        // so they hold no '"' or ';' of their own... except ';' itself).
        let mut v = Vec::new();
        let mut cur = String::new();
        let mut inq = false;
        for ch in list.chars() {
            match ch {
                '"' => {
                    inq = !inq;
                    cur.push(ch);
                }
                ';' if !inq => v.push(std::mem::take(&mut cur)),
                _ => cur.push(ch),
            }
        }
        if !cur.is_empty() {
            v.push(cur);
        }
        Some((&s[..start], v))
    }
    let (Some((wh, mut we)), Some((_, ge))) = (split_errs(want), split_errs(got)) else {
        return want.to_string();
    };
    if we.len() != ge.len() {
        return want.to_string();
    }
    for (w, g) in we.iter_mut().zip(&ge) {
        if g.ends_with(",\"?\"") {
            // Keeps "Type,failure" and blanks the context.
            let mut parts = w.splitn(3, ',');
            let t = parts.next().unwrap_or("").to_string();
            let f = parts.next().unwrap_or("").to_string();
            *w = format!("{t},{f},\"?\"");
        }
    }
    format!("{wh} [{}]", we.join(";"))
}

#[test]
fn whatwg_setters_php85_fixture() {
    run_setters("whatwg_setters_php85.txt");
}

/// Seeded random withers (`tests/fixtures/gen_fuzz.py`).
#[test]
fn whatwg_setters_php85_fuzz_fixture() {
    run_setters("whatwg_setters_fuzz_php85.txt");
}

fn run_setters(file: &'static str) {
    let mut rep = Report::new(file);
    for f in fixture(file) {
        let line = f[..3].join("\t");
        let input = unhex(&f[0]);
        let arg = &f[2];
        let sval: Option<Vec<u8>> = arg.strip_prefix('S').map(unhex);
        let s = sval.as_deref();
        let ival: Option<i64> = arg.strip_prefix('I').map(|v| v.parse().unwrap());
        let got = match Url::parse(&input, None) {
            Err(_) => "PARSEFAIL".to_string(),
            Ok((u, _)) => {
                let r = match f[1].as_str() {
                    "withScheme" => u.with_scheme(s.unwrap()),
                    "withUsername" => u.with_username(s),
                    "withPassword" => u.with_password(s),
                    "withHost" => u.with_host(s),
                    "withPort" => u.with_port(ival),
                    "withPath" => u.with_path(s.unwrap()),
                    "withQuery" => u.with_query(s),
                    "withFragment" => u.with_fragment(s),
                    m => unreachable!("{m}"),
                };
                match r {
                    Ok(v) => wurl(&v),
                    Err(mut e) => {
                        // PHP shows the inherited errors' contexts from
                        // freed memory: compare their types only.
                        let n = e.errors.len();
                        for x in &mut e.errors[n - e.inherited..] {
                            x.context = b"?".to_vec();
                        }
                        exc(&e)
                    }
                }
            }
        };
        let want = blank_inherited(&f[3..].join("\t"), &got);
        rep.check(|| format!("{line}  {} {}", show(&input), arg), &got, &want);
    }
    rep.finish();
}
