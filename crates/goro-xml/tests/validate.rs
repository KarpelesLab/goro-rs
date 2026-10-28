//! DTD validation against fixtures recorded on PHP 8.5
//! (tests/fixtures/validate.php). Columns of LIBXML_DTDVALID errors are
//! parse positions and are not compared.

mod common;
use common::*;
use goro_xml::Arena;
use goro_xml::xml::{ParseOptions, build, opt, validate};

#[test]
fn validate_fixtures() {
    let text = fixture("validate.out");
    let mut cases: Vec<(Vec<u8>, String, bool, Vec<String>)> = Vec::new();
    for line in text.lines() {
        let (tag, rest) = line.split_once(' ').unwrap_or((line, ""));
        match tag {
            "C" => {
                let (i, m) = rest.split_once(' ').unwrap();
                cases.push((b64(i), m.to_string(), false, Vec::new()));
            }
            "R" => cases.last_mut().unwrap().2 = rest == "1",
            "E" => {
                let f: Vec<&str> = rest.split(' ').collect();
                let msg = String::from_utf8_lossy(&b64(f[4])).trim_end().to_string();
                let c = cases.last_mut().unwrap();
                if c.1 == "validate" {
                    c.3.push(format!("{} {} {} {}", f[0], f[1], f[2], msg));
                } else {
                    c.3.push(format!("{} {} {}", f[0], f[1], msg));
                }
            }
            _ => {}
        }
    }
    let mut fails = 0;
    for (input, mode, result, errs) in &cases {
        let mut arena = Arena::new();
        let (ok, got): (bool, Vec<String>) = if mode == "validate" {
            let b = build(&mut arena, input, &ParseOptions::default(), None);
            let v = validate::validate(&arena, b.doc);
            let lines = v
                .iter()
                .map(|e| {
                    format!(
                        "{} {} {} {}",
                        e.level,
                        e.code,
                        if e.line == 0 && e.code == 535 {
                            -1
                        } else {
                            e.line as i64
                        },
                        e.text().trim_end()
                    )
                })
                .collect();
            (v.is_empty(), lines)
        } else {
            let b = build(
                &mut arena,
                input,
                &ParseOptions::with_flags(opt::DTDVALID),
                None,
            );
            (
                b.ok,
                b.errors
                    .iter()
                    .map(|e| format!("{} {} {}", e.level, e.code, e.text().trim_end()))
                    .collect(),
            )
        };
        if ok != *result || &got != errs {
            fails += 1;
            eprintln!(
                "--- {:?} {mode}\n  got  {ok} {got:?}\n  want {result} {errs:?}",
                show(input)
            );
        }
    }
    eprintln!("validate: {}/{} pass", cases.len() - fails, cases.len());
    assert_eq!(fails, 0);
}
