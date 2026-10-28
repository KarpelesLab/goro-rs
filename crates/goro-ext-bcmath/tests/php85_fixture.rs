//! Replays `fixtures/bcmath_php85.txt`, generated once on PHP 8.5 by
//! `fixtures/bcmath_driver.php` (see that file for the line format).

use goro_ext_bcmath::{
    BcError, BinOp, Number, NumberArg, RoundingMode, bcadd, bcceil, bccomp, bcdiv, bcdivmod,
    bcfloor, bcmod, bcmul, bcpow, bcpowmod, bcround, bcscale, bcsqrt, bcsub,
};
use std::cmp::Ordering;

/// Rebuilds a large operand from its `@seed,int,frac,neg` token (the
/// driver's `gen()`).
fn big_operand(token: &str) -> Vec<u8> {
    let v: Vec<u64> = token.split(',').map(|x| x.parse().unwrap()).collect();
    let (mut x, il, fl, neg) = (v[0], v[1], v[2], v[3]);
    let mut s = Vec::new();
    if neg == 1 {
        s.push(b'-');
    }
    for i in 0..il + fl {
        x = (x * 1_103_515_245 + 12_345) % 2_147_483_648;
        let mut d = (x >> 16) % 10;
        if i == 0 && d == 0 {
            d = 1;
        }
        if i == il {
            s.push(b'.');
        }
        s.push(b'0' + d as u8);
    }
    s
}

fn unescape(s: &str) -> Vec<u8> {
    if let Some(t) = s.strip_prefix("@") {
        return big_operand(t);
    }
    if let Some(t) = s.strip_prefix("s:@") {
        let mut v = b"s:".to_vec();
        v.extend(big_operand(t));
        return v;
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() + 1 && b.get(i + 1) == Some(&b'x') {
            let h = std::str::from_utf8(&b[i + 2..i + 4]).unwrap();
            out.push(u8::from_str_radix(h, 16).unwrap());
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

fn escape(s: &[u8]) -> String {
    let mut out = String::with_capacity(s.len());
    for &c in s {
        if (0x21..=0x7e).contains(&c) && c != b'\\' {
            out.push(c as char);
        } else {
            out.push_str(&format!("\\x{c:02x}"));
        }
    }
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A result rendered like the driver's `res()`, before escaping.
fn finish(raw: Vec<u8>) -> String {
    if raw.len() > 300 {
        format!("#{}:{:08x}", raw.len(), crc32(&raw))
    } else {
        escape(&raw)
    }
}

fn s_res(v: Vec<u8>) -> Vec<u8> {
    let mut r = b"=".to_vec();
    r.extend_from_slice(&v);
    r
}

fn n_res(n: &Number) -> Vec<u8> {
    format!("N({},{})", n.as_str(), n.scale()).into_bytes()
}

fn err(e: BcError) -> String {
    format!("!{}: {}", e.class_name(), escape(e.message.as_bytes()))
}

fn render(r: Result<Vec<u8>, BcError>) -> String {
    match r {
        Ok(v) => finish(v),
        Err(e) => err(e),
    }
}

fn scale_arg(s: &[u8]) -> Option<i64> {
    if s == b"~" {
        None
    } else {
        Some(std::str::from_utf8(s).unwrap().parse().unwrap())
    }
}

fn int(s: &[u8]) -> i64 {
    std::str::from_utf8(s).unwrap().parse().unwrap()
}

/// A typed operand: `s:`, `i:` or `n:`.
enum Typed {
    S(Vec<u8>),
    I(i64),
    N(Number),
}

fn typed(s: &[u8]) -> Typed {
    let (k, v) = s.split_at(2);
    match k {
        b"i:" => Typed::I(int(v)),
        b"n:" => Typed::N(Number::parse(v).expect("fixture Number")),
        _ => Typed::S(v.to_vec()),
    }
}

impl Typed {
    fn arg(&self) -> NumberArg<'_> {
        match self {
            Typed::S(s) => NumberArg::Str(s),
            Typed::I(i) => NumberArg::Int(*i),
            Typed::N(n) => NumberArg::Number(n),
        }
    }
}

fn mode(s: &[u8]) -> RoundingMode {
    if s == b"~" {
        RoundingMode::HalfAwayFromZero
    } else {
        RoundingMode::from_name(std::str::from_utf8(s).unwrap()).unwrap()
    }
}

fn num_result(r: Result<Number, BcError>) -> String {
    render(r.map(|n| n_res(&n)))
}

/// Strips `{...}` notice prefixes (engine diagnostics, e.g. the `%`
/// operator's int conversion of a float-string left operand).
fn strip_notes(mut s: &str) -> &str {
    while s.starts_with('{') {
        match s.find('}') {
            Some(i) => s = &s[i + 1..],
            None => break,
        }
    }
    s
}

#[test]
fn php85_fixture() {
    let text = include_str!("fixtures/bcmath_php85.txt");
    let mut default_scale: u32 = 0;
    let mut failures = Vec::new();
    let mut count = 0;
    let mut skipped = 0;
    for line in text.lines() {
        let Some((lhs, expected)) = line.split_once("\t=>\t") else {
            continue;
        };
        let expected = strip_notes(expected);
        let mut parts = lhs.split('\t');
        let op = parts.next().unwrap();
        let args: Vec<Vec<u8>> = parts.map(unescape).collect();
        let a = |i: usize| -> &[u8] { &args[i] };
        let got: String = match op {
            "scale" => {
                let old = default_scale;
                default_scale = int(a(0)) as u32;
                format!("={old}")
            }
            "add" | "sub" | "mul" | "div" | "mod" | "comp" | "divmod" => {
                let s = scale_arg(a(2));
                match op {
                    "add" => render(bcadd(a(0), a(1), s, default_scale).map(s_res)),
                    "sub" => render(bcsub(a(0), a(1), s, default_scale).map(s_res)),
                    "mul" => render(bcmul(a(0), a(1), s, default_scale).map(s_res)),
                    "div" => render(bcdiv(a(0), a(1), s, default_scale).map(s_res)),
                    "mod" => render(bcmod(a(0), a(1), s, default_scale).map(s_res)),
                    "comp" => render(
                        bccomp(a(0), a(1), s, default_scale)
                            .map(|v| s_res(v.to_string().into_bytes())),
                    ),
                    _ => match bcdivmod(a(0), a(1), s, default_scale) {
                        Ok((q, r)) => format!("[{}|{}]", finish(s_res(q)), finish(s_res(r))),
                        Err(e) => err(e),
                    },
                }
            }
            "pow" => render(bcpow(a(0), a(1), scale_arg(a(2)), default_scale).map(s_res)),
            "sqrt" => render(bcsqrt(a(0), scale_arg(a(1)), default_scale).map(s_res)),
            "powmod" => {
                render(bcpowmod(a(0), a(1), a(2), scale_arg(a(3)), default_scale).map(s_res))
            }
            "round" => render(bcround(a(0), int(a(1)), mode(a(2))).map(s_res)),
            "floor" => render(bcfloor(a(0)).map(s_res)),
            "ceil" => render(bcceil(a(0)).map(s_res)),
            "bcscale" => {
                let old = default_scale;
                match bcscale(Some(int(a(0)))) {
                    Ok(new) => {
                        if let Some(n) = new {
                            default_scale = n;
                        }
                        format!("={old}")
                    }
                    Err(e) => err(e),
                }
            }
            "ctor" => {
                let t = typed(a(0));
                num_result(Number::new(t.arg()))
            }
            "unser" => {
                let v = a(0);
                let value = if v == b"~" || v.starts_with(b"i:") {
                    None
                } else {
                    Some(v)
                };
                num_result(Number::unserialize(value))
            }
            "N.sqrt" => {
                let n = Number::parse(a(0)).unwrap();
                num_result(n.sqrt(scale_arg(a(1))))
            }
            "N.round" => {
                let n = Number::parse(a(0)).unwrap();
                num_result(n.round(int(a(1)), mode(a(2))))
            }
            "N.floor" => num_result(Number::parse(a(0)).unwrap().floor()),
            "N.ceil" => num_result(Number::parse(a(0)).unwrap().ceil()),
            "N.powmod" => {
                let n = Number::parse(a(0)).unwrap();
                let e = typed(a(1));
                let m = typed(a(2));
                num_result(n.powmod(e.arg(), m.arg(), scale_arg(a(3))))
            }
            m if m.starts_with("N.") => {
                let n = Number::parse(a(0)).unwrap();
                let b = typed(a(1));
                let s = scale_arg(a(2));
                match &m[2..] {
                    "add" => num_result(n.add(b.arg(), s)),
                    "sub" => num_result(n.sub(b.arg(), s)),
                    "mul" => num_result(n.mul(b.arg(), s)),
                    "div" => num_result(n.div(b.arg(), s)),
                    "mod" => num_result(n.modulo(b.arg(), s)),
                    "pow" => num_result(n.pow(b.arg(), s)),
                    "compare" => render(
                        n.compare(b.arg(), s)
                            .map(|v| s_res(v.to_string().into_bytes())),
                    ),
                    "divmod" => match n.divmod(b.arg(), s) {
                        Ok((q, r)) => format!("[{}|{}]", finish(n_res(&q)), finish(n_res(&r))),
                        Err(e) => err(e),
                    },
                    other => panic!("unknown method {other}"),
                }
            }
            o if o.starts_with("op") => {
                if expected.starts_with("!TypeError") {
                    // The engine's int check of a `%` operand, not bcmath.
                    skipped += 1;
                    continue;
                }
                let l = typed(a(0));
                let r = typed(a(1));
                let bop = match &o[2..] {
                    "+" => BinOp::Add,
                    "-" => BinOp::Sub,
                    "*" => BinOp::Mul,
                    "/" => BinOp::Div,
                    "%" => BinOp::Mod,
                    "**" => BinOp::Pow,
                    other => panic!("unknown operator {other}"),
                };
                num_result(Number::binary_op(bop, l.arg(), r.arg()))
            }
            "cmp" => {
                let l = typed(a(0));
                let r = typed(a(1));
                let o = Number::compare_operands(l.arg(), r.arg());
                let t = |b: bool| if b { 'T' } else { 'F' };
                match o {
                    None => "=1,F,F,F".to_string(),
                    Some(o) => format!(
                        "={},{},{},{}",
                        match o {
                            Ordering::Less => -1,
                            Ordering::Equal => 0,
                            Ordering::Greater => 1,
                        },
                        t(o == Ordering::Equal),
                        t(o == Ordering::Less),
                        t(o == Ordering::Greater)
                    ),
                }
            }
            "bool" => {
                let n = Number::parse(a(0)).unwrap();
                format!("={}", if n.to_bool() { 'T' } else { 'F' })
            }
            "neg" => num_result(Ok(Number::parse(a(0)).unwrap().negate())),
            "inc" => num_result(Number::parse(a(0)).unwrap().increment()),
            "dec" => num_result(Number::parse(a(0)).unwrap().decrement()),
            other => panic!("unknown fixture op {other}"),
        };
        count += 1;
        if got != expected {
            failures.push(format!(
                "{lhs}\n    expected {expected}\n    got      {got}"
            ));
        }
    }
    eprintln!(
        "{count} cases, {} failures, {skipped} skipped",
        failures.len()
    );
    assert!(count > 20_000, "fixture too small: {count}");
    if !failures.is_empty() {
        let shown: Vec<_> = failures.iter().take(60).cloned().collect();
        panic!(
            "{} of {count} fixture cases differ ({skipped} skipped):\n{}",
            failures.len(),
            shown.join("\n")
        );
    }
}

/// Squares of all-nines numbers: 9..9 (8n digits) squared is 9..98 0..01
/// (test ext/bcmath/tests/bcmul_check_overflow.phpt).
#[test]
fn mul_all_nines() {
    for i in 1..13 {
        let repeat = 1usize << i;
        let num = "99999999".repeat(repeat);
        let want = format!(
            "{}8{}1",
            "9".repeat(repeat * 8 - 1),
            "0".repeat(repeat * 8 - 1)
        );
        let got = bcmul(num.as_bytes(), num.as_bytes(), None, 0).unwrap();
        assert!(got == want.as_bytes(), "repeat {repeat}");
    }
}

/// Large exponents whose results truncate to (or near) zero, observed on
/// PHP 8.5 (`bcpow()` computes exactly; these must not build the full power).
#[test]
fn pow_large_exponents() {
    for (b, e, s, want) in [
        ("0.5", "1000000", 5, "0.00000"),
        ("3", "-1000000", 10, "0.0000000000"),
        ("0.99", "100000", 3, "0.000"),
        ("-0.5", "99999", 2, "0.00"),
        ("1.0000001", "-100000", 8, "0.99004983"),
        ("0.000001", "50000", 3, "0.000"),
        ("-7", "-99999", 0, "0"),
    ] {
        let got = bcpow(b.as_bytes(), e.as_bytes(), Some(s), 0).unwrap();
        assert_eq!(String::from_utf8(got).unwrap(), want, "{b} ** {e} @{s}");
    }
}
