//! Checks every line of tests/fixtures/filter_php85.txt, the output of
//! tests/fixtures/filter_driver.php on PHP 8.5 (see the driver for the line
//! formats).

mod common;

use std::collections::BTreeMap;

use common::{Exc, Host, esc, serialize, unesc, unserialize};
use goro_ext_filter::{Options, filter_var, filter_var_array, valid_email};
use goro_runtime::Value;
use goro_runtime::compare::strict_eq;

/// The driver's `result()` for one call.
fn show(host: &mut Host, input: &Value, r: Result<Value, Exc>) -> String {
    let mut out = match r {
        Ok(v) => {
            if strict_eq(&v, input) {
                "=".to_string()
            } else {
                match v {
                    Value::Bool(false) => "F".to_string(),
                    Value::Bool(true) => "T".to_string(),
                    Value::Null => "N".to_string(),
                    v => format!("R{}", esc(&serialize(&v))),
                }
            }
        }
        Err(e) => {
            let (class, msg) = e.describe();
            format!("X{class}:{}", esc(&msg))
        }
    };
    for w in host.take_warnings() {
        out.push_str(" W");
        out.push_str(&esc(w.as_bytes()));
    }
    out
}

fn options(v: &Value) -> Options<'_> {
    match v {
        Value::Int(i) => Options::Int(*i),
        Value::Array(a) => Options::Array(a),
        _ => panic!("options must be int or array"),
    }
}

#[test]
fn php85_fixture() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/filter_php85.txt"
    ))
    .unwrap();
    let mut host = Host::new();
    let mut opts: Vec<Value> = Vec::new();
    let mut input = Value::Null;
    let mut input_field = String::new();
    let mut checked = 0usize;
    let mut failures: BTreeMap<String, (usize, Vec<String>)> = BTreeMap::new();
    let mut saw_end = false;
    for line in text.lines() {
        let t: Vec<&str> = line.split(' ').collect();
        match t[0] {
            "O" => {
                let n: usize = t[1].parse().unwrap();
                assert_eq!(n, opts.len());
                opts.push(unserialize(&host, &unesc(t[2])));
            }
            "I" => {
                input = Value::Str(unesc(t[1]).as_slice().into());
                input_field = line.to_string();
            }
            "J" => {
                input = unserialize(&host, &unesc(t[1]));
                input_field = line.to_string();
            }
            "V" | "A" => {
                let expected = t[3..].join(" ");
                let (key, got) = if t[0] == "V" {
                    let filter: i64 = t[1].parse().unwrap();
                    let o = &opts[t[2].parse::<usize>().unwrap()];
                    let r = filter_var(&mut host, &input, filter, options(o));
                    (format!("V {filter}"), show(&mut host, &input, r))
                } else {
                    let add_empty = t[2] == "1";
                    let Value::Array(a) = &input else {
                        panic!("A needs an array")
                    };
                    let r = match t[1].strip_prefix('o') {
                        Some(n) => {
                            let o = &opts[n.parse::<usize>().unwrap()];
                            filter_var_array(&mut host, a, options(o), add_empty)
                        }
                        None => filter_var_array(
                            &mut host,
                            a,
                            Options::Int(t[1].parse().unwrap()),
                            add_empty,
                        ),
                    };
                    ("A".to_string(), show(&mut host, &input, r))
                };
                checked += 1;
                if got != expected {
                    let e = failures.entry(key).or_default();
                    e.0 += 1;
                    if e.1.len() < 12 {
                        e.1.push(format!("{input_field}\n    {line}\n    got {got}"));
                    }
                }
            }
            "U" => {
                let first = u32::from_str_radix(t[1], 16).unwrap();
                let last = u32::from_str_radix(t[2], 16).unwrap();
                let want = t[3];
                for cp in first..=last {
                    let ch = char::from_u32(cp).unwrap().to_string();
                    let atom = format!("a{ch}b@b.cd");
                    let quoted = format!("\"a{ch}b\"@b.cd");
                    let got = format!(
                        "{}{}",
                        valid_email(atom.as_bytes(), true) as u8,
                        valid_email(quoted.as_bytes(), true) as u8
                    );
                    checked += 1;
                    if got != want {
                        let e = failures.entry("U".into()).or_default();
                        e.0 += 1;
                        if e.1.len() < 12 {
                            e.1.push(format!("U+{cp:04X}: want {want} got {got}"));
                        }
                    }
                }
            }
            "END" => saw_end = true,
            _ => panic!("bad line {line}"),
        }
    }
    assert!(saw_end, "truncated fixture");
    if !failures.is_empty() {
        let mut report = String::new();
        let mut total = 0;
        for (k, (n, samples)) in &failures {
            total += n;
            report.push_str(&format!("== {k}: {n} mismatches\n"));
            for s in samples {
                report.push_str(&format!("  {s}\n"));
            }
        }
        panic!("{total} of {checked} cases differ:\n{report}");
    }
    eprintln!("{checked} cases match");
}
