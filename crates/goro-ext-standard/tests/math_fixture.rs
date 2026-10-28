//! Checks the math functions against `fixtures/math.txt`, the output of
//! `fixtures/math.php` on PHP 8.5.

use goro_ext_standard::error::{Diag, StdError};
use goro_ext_standard::math::{self, Number, RoundMode};

#[derive(Debug, Clone)]
enum V {
    I(i64),
    F(f64),
    S(Vec<u8>),
    M(String),
    N,
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|c| format!("{c:02x}")).collect()
}

fn val(s: &str) -> V {
    let (t, v) = s.split_once(':').unwrap();
    match t {
        "i" => V::I(v.parse().unwrap()),
        "f" => V::F(f64::from_bits(u64::from_str_radix(v, 16).unwrap())),
        "s" => V::S(unhex(v)),
        "m" => V::M(v.to_string()),
        "n" => V::N,
        _ => V::N,
    }
}

fn f(v: &V) -> f64 {
    match v {
        V::I(i) => *i as f64,
        V::F(f) => *f,
        _ => panic!("{v:?}"),
    }
}

fn i(v: &V) -> i64 {
    match v {
        V::I(i) => *i,
        _ => panic!("{v:?}"),
    }
}

fn s(v: &V) -> Vec<u8> {
    match v {
        V::S(s) => s.clone(),
        V::N => Vec::new(),
        _ => panic!("{v:?}"),
    }
}

fn enc_f(x: f64) -> String {
    format!("R:f:{:016x}", x.to_bits())
}

fn enc_num(n: Number) -> String {
    match n {
        Number::Int(i) => format!("R:i:{i}"),
        Number::Float(x) => enc_f(x),
    }
}

fn enc_err(e: StdError) -> String {
    match e {
        StdError::ValueError(m) => format!("E:ValueError:{m}"),
        StdError::DivisionByZeroError(m) => format!("E:DivisionByZeroError:{m}"),
        StdError::ArithmeticError(m) => format!("E:ArithmeticError:{m}"),
        e => format!("{e:?}"),
    }
}

fn with_diags(r: String, d: &[Diag]) -> String {
    let mut r = r;
    for x in d {
        match x {
            Diag::Deprecated(m) | Diag::Warning(m) | Diag::Notice(m) => {
                r.push_str("\tW:");
                r.push_str(m);
            }
        }
    }
    r
}

fn eval(name: &str, a: &[V]) -> Option<String> {
    let mut d = Vec::new();
    let r = match name {
        "round" => {
            let mode = match a.get(2) {
                None => RoundMode::HalfAwayFromZero,
                Some(V::I(m)) => match RoundMode::from_int(*m) {
                    Ok(m) => m,
                    Err(e) => return Some(enc_err(e)),
                },
                Some(V::M(n)) => RoundMode::from_case_name(n).unwrap(),
                Some(v) => panic!("{v:?}"),
            };
            enc_f(math::round(f(&a[0]), a.get(1).map_or(0, i), mode))
        }
        n if n.starts_with("round_") => {
            let mode = RoundMode::from_case_name(&n[6..]).unwrap();
            enc_f(math::round(f(&a[0]), i(&a[1]), mode))
        }
        "ceil" => enc_f(f(&a[0]).ceil()),
        "floor" => enc_f(f(&a[0]).floor()),
        "fmod" => enc_f(math::fmod(f(&a[0]), f(&a[1]))),
        "intdiv" => match math::intdiv(i(&a[0]), i(&a[1])) {
            Ok(v) => format!("R:i:{v}"),
            Err(e) => enc_err(e),
        },
        "number_format" => {
            let dec = a.get(1).map_or(0, i);
            let dp = a.get(2).map_or(b".".to_vec(), s);
            let ts = a.get(3).map_or(b",".to_vec(), s);
            let dp = if matches!(a.get(2), Some(V::N)) {
                b".".to_vec()
            } else {
                dp
            };
            let ts = if matches!(a.get(3), Some(V::N)) {
                b",".to_vec()
            } else {
                ts
            };
            let out = match &a[0] {
                V::I(n) => math::number_format_int(*n, dec, &dp, &ts),
                v => math::number_format(f(v), dec, &dp, &ts),
            };
            format!("R:s:{}", hex(&out))
        }
        "hexdec" => enc_num(math::parse_base(&s(&a[0]), 16, &mut d)),
        "octdec" => enc_num(math::parse_base(&s(&a[0]), 8, &mut d)),
        "bindec" => enc_num(math::parse_base(&s(&a[0]), 2, &mut d)),
        "base_convert" => match math::base_convert(&s(&a[0]), i(&a[1]), i(&a[2]), &mut d) {
            Ok(v) => format!("R:s:{}", hex(&v)),
            Err(e) => enc_err(e),
        },
        "decbin" => format!("R:s:{}", hex(&math::dec_to_base(i(&a[0]), 2))),
        "dechex" => format!("R:s:{}", hex(&math::dec_to_base(i(&a[0]), 16))),
        "decoct" => format!("R:s:{}", hex(&math::dec_to_base(i(&a[0]), 8))),
        _ => return None,
    };
    Some(with_diags(r, &d))
}

#[test]
fn math_fixture() {
    let data = include_str!("fixtures/math.txt");
    let mut failures = Vec::new();
    let mut n = 0;
    for line in data.lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        let args: Vec<V> = if parts[1].is_empty() {
            Vec::new()
        } else {
            parts[1].split(',').map(val).collect()
        };
        let Some(got) = eval(parts[0], &args) else {
            continue;
        };
        n += 1;
        if got != parts[2] {
            failures.push(format!(
                "{} {:?}\n  want {}\n  got  {}",
                parts[0], args, parts[2], got
            ));
        }
    }
    assert!(n > 7000, "{n}");
    if !failures.is_empty() {
        panic!(
            "{} of {} cases differ:\n{}",
            failures.len(),
            n,
            failures[..failures.len().min(60)].join("\n")
        );
    }
}
