//! DOMDocument::loadXML()/saveXML() behaviour against fixtures recorded on
//! PHP 8.5 (tests/fixtures/loadxml.php).

mod common;
use common::*;
use goro_xml::Arena;
use goro_xml::serialize::{SaveOptions, save_xml};
use goro_xml::xml::{ParseOptions, build, opt};

struct Case {
    input: Vec<u8>,
    opts: u32,
    fmt: bool,
    save: u32,
    pws: bool,
    result: bool,
    errors: Vec<String>,
    out: Option<Vec<u8>>,
}

fn cases(name: &str) -> Vec<Case> {
    let mut v: Vec<Case> = Vec::new();
    for line in fixture(name).lines() {
        let mut parts = line.splitn(2, ' ');
        let tag = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("");
        match tag {
            "C" => {
                let f: Vec<&str> = rest.split(' ').collect();
                v.push(Case {
                    input: b64(f[0]),
                    opts: f[1].parse().unwrap(),
                    fmt: f[2] == "1",
                    save: f[3].parse().unwrap(),
                    pws: f[4] == "1",
                    result: false,
                    errors: Vec::new(),
                    out: None,
                });
            }
            "R" => v.last_mut().unwrap().result = rest == "1",
            "E" => {
                let f: Vec<&str> = rest.split(' ').collect();
                let msg = String::from_utf8_lossy(&b64(f[4])).into_owned();
                v.last_mut().unwrap().errors.push(format!(
                    "{} {} {} {} {}",
                    f[0],
                    f[1],
                    f[2],
                    f[3],
                    msg.trim_end()
                ));
            }
            "O" => v.last_mut().unwrap().out = Some(b64(rest)),
            _ => {}
        }
    }
    v
}

#[test]
fn loadxml_fixtures() {
    let mut fails = 0;
    let all = cases("loadxml.out");
    for (i, c) in all.iter().enumerate() {
        // The reference build resolves the XHTML DTD through a system
        // catalog and adds its `xmlns` default: build-specific, not copied.
        if c.input.windows(9).any(|w| w == b"DTD XHTML") {
            continue;
        }
        let mut arena = Arena::new();
        let mut flags = c.opts;
        if !c.pws {
            flags |= opt::NOBLANKS;
        }
        let po = ParseOptions::with_flags(flags);
        let b = build(&mut arena, &c.input, &po, None);
        let errs: Vec<String> = b
            .errors
            .iter()
            .map(|e| {
                format!(
                    "{} {} {} {} {}",
                    e.level,
                    e.code,
                    e.line,
                    e.column,
                    e.message.trim_end()
                )
            })
            .collect();
        let out = if b.ok {
            Some(save_xml(&arena, b.doc, &SaveOptions::new(c.fmt, c.save)))
        } else {
            None
        };
        let ok = b.ok == c.result && errs == c.errors && out == c.out;
        if !ok {
            fails += 1;
            eprintln!("--- case {i}: {:?} opts={}", show(&c.input), c.opts);
            if b.ok != c.result {
                eprintln!("  result: got {} want {}", b.ok, c.result);
            }
            if errs != c.errors {
                eprintln!("  errors got:  {errs:?}\n  errors want: {:?}", c.errors);
            }
            if out != c.out {
                eprintln!(
                    "  out got:  {:?}\n  out want: {:?}",
                    out.as_deref().map(show),
                    c.out.as_deref().map(show)
                );
            }
        }
    }
    eprintln!("loadxml: {}/{} pass", all.len() - fails, all.len());
    assert!(fails <= 0, "{fails} fixture cases differ");
}
