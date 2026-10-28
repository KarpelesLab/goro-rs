//! Checks the printf engine against `fixtures/printf.txt`, the output of
//! `fixtures/printf.php` on PHP 8.5.

use goro_ext_standard::error::{Diag, StdError};
use goro_ext_standard::printf::{Arg, sprintf};

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn arg(s: &str) -> Arg {
    let (t, v) = s.split_once(':').unwrap();
    match t {
        "i" => Arg::Int(v.parse().unwrap()),
        "f" => Arg::Float(f64::from_bits(u64::from_str_radix(v, 16).unwrap())),
        "s" => Arg::Str(unhex(v)),
        // Booleans and null convert like "1" and "" for every specifier.
        "b" => Arg::Str(if v == "1" { b"1".to_vec() } else { Vec::new() }),
        "n" => Arg::Str(Vec::new()),
        _ => panic!("bad arg {s}"),
    }
}

#[test]
fn printf_fixture() {
    let data = include_str!("fixtures/printf.txt");
    let mut failures = Vec::new();
    let mut n = 0;
    for line in data.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        let fmt = unhex(parts[0]);
        let args: Vec<Arg> = if parts[1].is_empty() {
            Vec::new()
        } else {
            parts[1].split(',').map(arg).collect()
        };
        let expected = parts[2];
        let got = match sprintf(&fmt, &args) {
            Ok((out, diags)) => {
                let mut s = format!("R:{}", hex(&out));
                for d in diags {
                    // The fixture records every diagnostic as W: (its
                    // error handler collects notices too).
                    let (Diag::Warning(m) | Diag::Notice(m)) = d else {
                        panic!()
                    };
                    s.push_str(&format!("\tW:sprintf(): {m}"));
                }
                s
            }
            Err(StdError::ValueError(m)) => format!("E:ValueError:{m}"),
            Err(StdError::ArgumentCountError(m)) => format!("E:ArgumentCountError:{m}"),
            Err(e) => format!("{e:?}"),
        };
        // Only sprintf's own warnings come from the engine; conversion
        // warnings belong to the wiring layer.
        let mut want = expected.to_string();
        for w in &parts[3..] {
            if w.starts_with("W:sprintf(): ") {
                want.push('\t');
                want.push_str(w);
            }
        }
        n += 1;
        if got != want {
            failures.push(format!(
                "{:?} {:?}\n  want {}\n  got  {}",
                String::from_utf8_lossy(&fmt),
                parts[1],
                show(&want),
                show(&got)
            ));
        }
    }
    assert!(n > 2000);
    if !failures.is_empty() {
        panic!(
            "{} of {} cases differ:\n{}",
            failures.len(),
            n,
            failures.join("\n")
        );
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|c| format!("{c:02x}")).collect()
}

fn show(s: &str) -> String {
    match s.strip_prefix("R:") {
        Some(r) => {
            let (h, rest) = r.split_once('\t').unwrap_or((r, ""));
            format!("{:?} {rest}", String::from_utf8_lossy(&unhex(h)))
        }
        None => s.to_string(),
    }
}
