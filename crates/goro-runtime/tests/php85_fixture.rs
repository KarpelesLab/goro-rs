//! Data-driven tests against `fixtures/php85_fixtures.txt`, generated once
//! by `fixtures/gen_fixtures.php` on the reference PHP 8.5 binary (see that
//! script for the format). Every row is a conversion or operator result,
//! with the diagnostics PHP raised while computing it.

use goro_runtime::compare::{compare, greater, greater_eq, less, less_eq, loose_eq, strict_eq};
use goro_runtime::convert::{float_to_string, to_array, to_bool, to_float, to_int, to_key, to_str};
use goro_runtime::numeric::is_numeric_str;
use goro_runtime::ops;
use goro_runtime::{Diagnostics, Key, KeyRef, PhpArray, PhpResult, PhpStr, Value};

const FIXTURE: &str = include_str!("fixtures/php85_fixtures.txt");

fn sections() -> Vec<(&'static str, Vec<&'static str>)> {
    let mut out: Vec<(&str, Vec<&str>)> = Vec::new();
    for line in FIXTURE.lines() {
        if let Some(name) = line.strip_prefix('@') {
            out.push((name, Vec::new()));
        } else if let Some(last) = out.last_mut() {
            last.1.push(line);
        }
    }
    out
}

fn section(name: &str) -> Vec<&'static str> {
    sections()
        .into_iter()
        .find(|s| s.0 == name)
        .unwrap_or_else(|| panic!("no section {name}"))
        .1
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|c| format!("{c:02x}")).collect()
}

/// Decodes one encoded value; returns the value and the rest of the input.
fn decode(s: &str) -> (Value, &str) {
    let end = |s: &str| s.find([',', '}', '=']).unwrap_or(s.len());
    if let Some(r) = s.strip_prefix('N') {
        return (Value::Null, r);
    }
    if let Some(r) = s.strip_prefix("b:") {
        return (Value::Bool(&r[..1] == "1"), &r[1..]);
    }
    if let Some(r) = s.strip_prefix("i:") {
        let e = end(r);
        return (Value::Int(r[..e].parse().unwrap()), &r[e..]);
    }
    if let Some(r) = s.strip_prefix("d:") {
        let bits = u64::from_str_radix(&r[..16], 16).unwrap();
        return (Value::Float(f64::from_bits(bits)), &r[16..]);
    }
    if let Some(r) = s.strip_prefix("s:") {
        let e = end(r);
        return (Value::str(&unhex(&r[..e])), &r[e..]);
    }
    if let Some(mut r) = s.strip_prefix("a:{") {
        let mut a = PhpArray::new();
        while !r.starts_with('}') {
            let (k, rest) = decode(r);
            let rest = rest.strip_prefix('=').unwrap();
            let (v, rest) = decode(rest);
            let key = match k {
                Value::Int(i) => Key::Int(i),
                Value::Str(s) => Key::Str(s),
                _ => panic!("bad key"),
            };
            a.insert(key, v);
            r = rest.strip_prefix(',').unwrap_or(rest);
        }
        return (Value::Array(a), &r[1..]);
    }
    panic!("cannot decode {s:?}")
}

fn dec(s: &str) -> Value {
    let (v, rest) = decode(s);
    assert!(rest.is_empty(), "trailing {rest:?} in {s:?}");
    v
}

fn enc(v: &Value) -> String {
    match v {
        Value::Null => "N".into(),
        Value::Bool(b) => format!("b:{}", *b as u8),
        Value::Int(i) => format!("i:{i}"),
        Value::Float(f) => format!("d:{:016x}", f.to_bits()),
        Value::Str(s) => format!("s:{}", hex(s)),
        Value::Array(a) => {
            let parts: Vec<String> = a
                .iter()
                .map(|(k, v)| format!("{}={}", enc(&k.to_value()), enc(v)))
                .collect();
            format!("a:{{{}}}", parts.join(","))
        }
        Value::Ref(r) => enc(&r.get()),
        _ => "?".into(),
    }
}

/// Encodes an outcome as the fixture does: diagnostics, tab, result.
fn outcome(ctx: &mut Diagnostics, r: PhpResult<Value>) -> String {
    let res = match r {
        Ok(v) => {
            // NAN payloads differ between producers; compare NANs as one.
            if let Value::Float(f) = v
                && f.is_nan()
            {
                return format!("{}\td:7ff8000000000000", diags(ctx));
            }
            enc(&v)
        }
        Err(e) => format!("T:{}:{}", e.kind.class_name(), e.message),
    };
    format!("{}\t{}", diags(ctx), res)
}

fn diags(ctx: &mut Diagnostics) -> String {
    let d = ctx.take();
    if d.is_empty() {
        "-".into()
    } else {
        d.iter()
            .map(|n| format!("{}:{}", n.level.errno(), n.message))
            .collect::<Vec<_>>()
            .join("|")
    }
}

fn normalize_nan(expected: &str) -> String {
    // PHP's NAN may be printed with either sign bit / payload.
    let mut parts: Vec<&str> = expected.splitn(2, '\t').collect();
    if parts.len() == 2
        && let Some(bits) = parts[1].strip_prefix("d:")
        && f64::from_bits(u64::from_str_radix(bits, 16).unwrap()).is_nan()
    {
        parts[1] = "d:7ff8000000000000";
    }
    parts.join("\t")
}

struct Report {
    name: &'static str,
    total: usize,
    failures: Vec<String>,
}

impl Report {
    fn new(name: &'static str) -> Report {
        Report {
            name,
            total: 0,
            failures: Vec::new(),
        }
    }
    fn check(&mut self, what: impl FnOnce() -> String, got: String, expected: &str) {
        self.total += 1;
        let expected = normalize_nan(expected);
        if got != expected {
            self.failures.push(format!(
                "{}\n    got:      {got}\n    expected: {expected}",
                what()
            ));
        }
    }
    fn finish(self) {
        if !self.failures.is_empty() {
            let shown: Vec<&String> = self.failures.iter().take(40).collect();
            panic!(
                "{}: {} of {} rows differ from PHP 8.5:\n{}",
                self.name,
                self.failures.len(),
                self.total,
                shown
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
        eprintln!("{}: {} rows match PHP 8.5", self.name, self.total);
    }
}

fn values(name: &str) -> Vec<Value> {
    section(name)
        .iter()
        .map(|l| {
            let (_, v) = l.split_once('\t').unwrap();
            dec(v)
        })
        .collect()
}

fn desc(v: &Value) -> String {
    match v {
        Value::Str(s) => format!("{:?}", s.to_string_lossy()),
        v => format!("{v:?}"),
    }
}

#[test]
fn conversions_and_unary_ops() {
    let vals = values("values");
    let mut rep = Report::new("convert");
    let mut ctx = Diagnostics::new();
    for line in section("convert") {
        let f: Vec<&str> = line.splitn(3, '\t').collect();
        let v = &vals[f[0].parse::<usize>().unwrap()];
        let op = f[1];
        let r: PhpResult<Value> = match op {
            "bool" => Ok(Value::Bool(to_bool(v, &mut ctx))),
            "int" => Ok(Value::Int(to_int(v, &mut ctx))),
            "float" => Ok(Value::Float(to_float(v, &mut ctx))),
            "string" => to_str(v, &mut ctx).map(Value::Str),
            "is_numeric" => Ok(Value::Bool(match v {
                Value::Int(_) | Value::Float(_) => true,
                Value::Str(s) => is_numeric_str(s),
                _ => false,
            })),
            "array" => Ok(Value::Array(to_array(v, &mut ctx))),
            "key" => to_key(v, "array", &mut ctx).map(|k| k.to_value()),
            "neg" => ops::neg(v, &mut ctx),
            "plus" => ops::plus(v, &mut ctx),
            "bitnot" => ops::bit_not(v, &mut ctx),
            "not" => Ok(Value::Bool(ops::not(v, &mut ctx))),
            "inc" => ops::increment(v, &mut ctx),
            "dec" => ops::decrement(v, &mut ctx),
            _ => panic!("unknown op {op}"),
        };
        let got = outcome(&mut ctx, r);
        rep.check(|| format!("{op}({})", desc(v)), got, f[2]);
    }
    rep.finish();
}

#[test]
fn binary_operators() {
    let vals = values("bvalues");
    let mut rep = Report::new("binop");
    let mut ctx = Diagnostics::new();
    for line in section("binop") {
        let f: Vec<&str> = line.splitn(4, '\t').collect();
        let a = &vals[f[0].parse::<usize>().unwrap()];
        let b = &vals[f[1].parse::<usize>().unwrap()];
        let op = f[2];
        let c = &mut ctx;
        let r: PhpResult<Value> = match op {
            "+" => ops::add(a, b, c),
            "-" => ops::sub(a, b, c),
            "*" => ops::mul(a, b, c),
            "/" => ops::div(a, b, c),
            "%" => ops::modulo(a, b, c),
            "**" => ops::pow(a, b, c),
            "intdiv" => ops::intdiv(a.as_int().unwrap(), b.as_int().unwrap()).map(Value::Int),
            "." => ops::concat(a, b, c).map(Value::Str),
            "&" => ops::bit_and(a, b, c),
            "|" => ops::bit_or(a, b, c),
            "^" => ops::bit_xor(a, b, c),
            "<<" => ops::shl(a, b, c),
            ">>" => ops::shr(a, b, c),
            "==" => loose_eq(a, b, c).map(Value::Bool),
            "===" => Ok(Value::Bool(strict_eq(a, b))),
            "<=>" => compare(a, b, c).map(|x| Value::Int(x as i64)),
            "<" => less(a, b, c).map(Value::Bool),
            "<=" => less_eq(a, b, c).map(Value::Bool),
            ">" => greater(a, b, c).map(Value::Bool),
            ">=" => greater_eq(a, b, c).map(Value::Bool),
            _ => panic!("unknown op {op}"),
        };
        let got = outcome(&mut ctx, r);
        rep.check(|| format!("{} {op} {}", desc(a), desc(b)), got, f[3]);
    }
    rep.finish();
}

#[test]
fn string_comparisons() {
    let vals = values("svalues");
    let mut rep = Report::new("scmp");
    let mut ctx = Diagnostics::new();
    for line in section("scmp") {
        let f: Vec<&str> = line.split('\t').collect();
        let a = &vals[f[0].parse::<usize>().unwrap()];
        let b = &vals[f[1].parse::<usize>().unwrap()];
        let eq = loose_eq(a, b, &mut ctx).unwrap() as i32;
        let cmp = compare(a, b, &mut ctx).unwrap();
        rep.check(
            || format!("{} vs {}", desc(a), desc(b)),
            format!("{eq}\t{cmp}"),
            &format!("{}\t{}", f[2], f[3]),
        );
    }
    rep.finish();
}

#[test]
fn float_to_string_precisions() {
    let mut rep = Report::new("floats");
    for line in section("floats") {
        let f: Vec<&str> = line.split('\t').collect();
        let x = f64::from_bits(u64::from_str_radix(f[0], 16).unwrap());
        for (i, p) in [14, 17, -1, 1, 5, 20].iter().enumerate() {
            let got = float_to_string(x, *p).to_string_lossy();
            rep.check(|| format!("{} at precision {p}", f[0]), got, f[i + 1]);
        }
    }
    rep.finish();
}

#[test]
fn numeric_string_parsing() {
    let mut rep = Report::new("parse");
    let mut ctx = Diagnostics::new();
    for line in section("parse") {
        let f: Vec<&str> = line.splitn(2, '\t').collect();
        let s = Value::Str(PhpStr::new(&unhex(f[0])));
        let r = ops::add(&s, &Value::Int(0), &mut ctx);
        let got = outcome(&mut ctx, r);
        rep.check(|| desc(&s), got, f[1]);
    }
    rep.finish();
}

#[test]
fn increment_decrement_strings() {
    let mut rep = Report::new("incdec");
    let mut ctx = Diagnostics::new();
    for line in section("incdec") {
        let f: Vec<&str> = line.splitn(3, '\t').collect();
        let s = Value::Str(PhpStr::new(&unhex(f[0])));
        let r = if f[1] == "inc" {
            ops::increment(&s, &mut ctx)
        } else {
            ops::decrement(&s, &mut ctx)
        };
        let got = outcome(&mut ctx, r);
        rep.check(|| format!("{}({})", f[1], desc(&s)), got, f[2]);
    }
    rep.finish();
}

#[test]
fn fixture_decoder_roundtrip() {
    let v = dec("a:{i:0=s:61,s:62=a:{i:1=N}}");
    assert_eq!(enc(&v), "a:{i:0=s:61,s:62=a:{i:1=N}}");
    let a = v.as_array().unwrap();
    assert!(a.get(KeyRef::Int(0)).is_some());
}
