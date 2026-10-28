//! Replays `fixtures/gmp_php85.txt`, generated once on PHP 8.5 (GMP 6.3)
//! by `fixtures/gmp_driver.php` (see that file for the line format).
//!
//! The harness plays the engine's part where PHP's behaviour is the
//! engine's: mapping values to [`Num`], the `int` pre-check of a non-object
//! left operand of `% & | ^ << >>`, comparisons between two objects whose
//! left one is not a GMP, `Stringable` arguments of `gmp_init()`, and the
//! `unserialize()` parser around the GMP payloads.

use std::cmp::Ordering;
use std::collections::HashMap;

use goro_ext_gmp::object::{self, UNSERIALIZE_NUMBER_ERROR, UNSERIALIZE_PROPERTIES_ERROR};
use goro_ext_gmp::*;
use goro_runtime::numeric::{Numericness, parse_numeric};
use goro_runtime::ops::BinOp;
use goro_runtime::{Ctx, Diagnostics, Level, Number};

#[derive(Clone, Debug)]
enum Arg {
    Int(i64),
    Str(Vec<u8>),
    Gmp(Gmp),
    Float(f64),
    Bool(bool),
    Null,
    Array,
    Obj,
    Stringable,
    Res,
}

impl Arg {
    fn num(&self) -> Num<'_> {
        match self {
            Arg::Int(i) => Num::Int(*i),
            Arg::Str(s) => Num::Str(s),
            Arg::Gmp(g) => Num::Gmp(g),
            Arg::Float(f) => Num::Float(*f),
            Arg::Bool(b) => Num::Bool(*b),
            Arg::Null => Num::Null,
            Arg::Array => Num::Other("array"),
            Arg::Obj => Num::Other("stdClass"),
            Arg::Stringable => Num::Other("T"),
            Arg::Res => Num::Other("resource"),
        }
    }
    fn int(&self) -> i64 {
        match self {
            Arg::Int(i) => *i,
            Arg::Bool(b) => *b as i64,
            a => panic!("not an int argument: {a:?}"),
        }
    }
    fn type_name(&self) -> &'static str {
        match self {
            Arg::Int(_) => "int",
            Arg::Str(_) => "string",
            Arg::Gmp(_) => "GMP",
            Arg::Float(_) => "float",
            Arg::Bool(_) => "bool",
            Arg::Null => "null",
            Arg::Array => "array",
            Arg::Obj => "stdClass",
            Arg::Stringable => "T",
            Arg::Res => "resource",
        }
    }
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|c| format!("{c:02x}")).collect()
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

fn long_or(prefix: &str, s: &str) -> String {
    if s.len() > 48 {
        format!("H{}:{:08x}", s.len(), crc32(s.as_bytes()))
    } else {
        format!("{prefix}{s}")
    }
}

fn dec(g: &Gmp) -> String {
    g.to_string_radix(10)
}

fn decode(tok: &str, defs: &HashMap<usize, Gmp>) -> Arg {
    let (k, v) = tok.split_at(1);
    match k {
        "i" => Arg::Int(v.parse().unwrap()),
        "q" => Arg::Str(v.as_bytes().to_vec()),
        "s" => Arg::Str(unhex(v)),
        "g" => Arg::Gmp(Gmp(parse_integer(v.as_bytes(), 10).unwrap())),
        "p" => Arg::Gmp(defs[&v.parse::<usize>().unwrap()].clone()),
        "f" => Arg::Float(f64::from_bits(u64::from_str_radix(v, 16).unwrap())),
        "b" => Arg::Bool(v == "1"),
        "n" => Arg::Null,
        "a" => Arg::Array,
        "o" => Arg::Obj,
        "t" => Arg::Stringable,
        "r" => Arg::Res,
        "R" => {
            // gmp_random_seed(seed); gmp_random_bits(bits), negated if asked
            let p: Vec<u64> = v.split(':').map(|x| x.parse().unwrap()).collect();
            goro_ext_gmp::random::seed(&Int::from_u64(p[0]));
            let x = goro_ext_gmp::random::bits(p[1]);
            Arg::Gmp(Gmp(if p[2] == 1 { x.neg() } else { x }))
        }
        "L" => {
            // the driver's lcg_digits()
            let p: Vec<u64> = v.split(':').map(|x| x.parse().unwrap()).collect();
            let digits = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
            let (mut x, len, base) = (p[0], p[1], p[2]);
            let s = (0..len)
                .map(|_| {
                    x = (x * 1_103_515_245 + 12_345) % 2_147_483_648;
                    digits[1 + ((x >> 16) % (base - 1)) as usize]
                })
                .collect();
            Arg::Str(s)
        }
        _ => panic!("bad token {tok}"),
    }
}

enum Out {
    G(Gmp),
    I(i64),
    B(bool),
    S(Vec<u8>),
    N,
    D(f64),
    A(Vec<Out>),
}

fn enc(o: &Out) -> String {
    match o {
        Out::G(g) => long_or("G", &dec(g)),
        Out::I(i) => format!("I{i}"),
        Out::B(b) => format!("B{}", *b as u8),
        Out::S(s) => long_or("S", &hex(s)),
        Out::N => "N".into(),
        Out::D(f) => format!("D{:016x}", f.to_bits()),
        Out::A(v) => format!("A({})", v.iter().map(enc).collect::<Vec<_>>().join(";")),
    }
}

fn err(e: GmpError) -> String {
    format!("E{}:{}", e.class_name(), e.message)
}

fn op_of(s: &str) -> Option<BinOp> {
    Some(match s {
        "+" => BinOp::Add,
        "-" => BinOp::Sub,
        "*" => BinOp::Mul,
        "/" => BinOp::Div,
        "%" => BinOp::Mod,
        "**" => BinOp::Pow,
        "&" => BinOp::BitAnd,
        "|" => BinOp::BitOr,
        "^" => BinOp::BitXor,
        "<<" => BinOp::Shl,
        ">>" => BinOp::Shr,
        _ => return None,
    })
}

/// The engine's int check of a non-object left operand of `%`, the
/// bitwise and the shift operators before the GMP handler runs (observed
/// on PHP 8.5; the same check as for `int % int`).
fn engine_int_precheck(ctx: &mut dyn Ctx, op: BinOp, a: &Arg) -> Result<(), String> {
    let unsupported = || {
        Err(format!(
            "ETypeError:Unsupported operand types: {} {} GMP",
            a.type_name(),
            op.symbol()
        ))
    };
    match a {
        Arg::Float(f) => {
            goro_runtime::convert::float_to_int_implicit(*f, ctx);
            Ok(())
        }
        Arg::Str(s) => {
            let p = parse_numeric(s);
            match p.kind {
                Numericness::NonNumeric => return unsupported(),
                Numericness::Leading => {
                    ctx.notice(Level::Warning, "A non-numeric value encountered".into())
                }
                Numericness::Numeric => {}
            }
            if let Number::Float(f) = p.num
                && (f.fract() != 0.0 || !f.is_finite())
            {
                ctx.notice(
                    Level::Deprecated,
                    format!(
                        "Implicit conversion from float-string \"{}\" to int loses precision",
                        String::from_utf8_lossy(s)
                    ),
                );
            }
            Ok(())
        }
        Arg::Array | Arg::Obj | Arg::Stringable | Arg::Res => unsupported(),
        _ => Ok(()),
    }
}

fn run_op(ctx: &mut Diagnostics, opname: &str, a: &Arg, b: &Arg) -> String {
    if let Some(op) = op_of(opname) {
        let a_obj = matches!(a, Arg::Gmp(_));
        if !a_obj
            && matches!(
                op,
                BinOp::Mod | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr
            )
            && let Err(e) = engine_int_precheck(ctx, op, a)
        {
            return e;
        }
        return match object::binary_op(ctx, op, a.num(), b.num()) {
            Some(Ok(g)) => enc(&Out::G(g)),
            Some(Err(e)) => err(e),
            None => "?".into(),
        };
    }
    // Comparisons. `$a > $b` is `$b < $a` in PHP.
    let (l, r, flip) = match opname {
        ">" | ">=" => (b, a, true),
        _ => (a, b, false),
    };
    // Two objects: PHP uses the left object's handler; a non-GMP one finds
    // them uncomparable (1).
    let ord = if matches!(l, Arg::Obj | Arg::Stringable) {
        Ok(Ordering::Greater)
    } else {
        object::compare(ctx, l.num(), r.num())
    };
    let ord = match ord {
        Ok(o) => o,
        Err(e) => return err(e),
    };
    let _ = flip;
    let uncomparable = matches!(l, Arg::Obj | Arg::Stringable);
    let out = match opname {
        "<=>" => Out::I(ord as i64),
        "==" => Out::B(ord == Ordering::Equal && !uncomparable),
        "!=" => Out::B(ord != Ordering::Equal || uncomparable),
        "<" | ">" => Out::B(ord == Ordering::Less),
        "<=" | ">=" => Out::B(ord != Ordering::Greater),
        _ => panic!("op {opname}"),
    };
    enc(&out)
}

// --------------------------------------------------- unserialize emulation

#[derive(Debug)]
enum SVal {
    Int(i64),
    Str(Vec<u8>),
    Float(f64),
    Null,
    Bool(bool),
    Array(#[allow(dead_code)] Vec<(SVal, SVal)>),
}

fn parse_sval(s: &[u8], pos: &mut usize) -> Option<SVal> {
    let t = *s.get(*pos)?;
    let read_until = |pos: &mut usize, end: u8| -> Option<String> {
        let start = *pos;
        while *s.get(*pos)? != end {
            *pos += 1;
        }
        let v = String::from_utf8(s[start..*pos].to_vec()).ok()?;
        *pos += 1;
        Some(v)
    };
    match t {
        b'N' => {
            *pos += 2;
            Some(SVal::Null)
        }
        b'i' | b'd' | b'b' => {
            *pos += 2;
            let v = read_until(pos, b';')?;
            Some(match t {
                b'i' => SVal::Int(v.parse().ok()?),
                b'd' => SVal::Float(v.parse().ok()?),
                _ => SVal::Bool(v == "1"),
            })
        }
        b's' => {
            *pos += 2;
            let n: usize = read_until(pos, b':')?.parse().ok()?;
            *pos += 1; // "
            let v = s.get(*pos..*pos + n)?.to_vec();
            *pos += n + 2; // ";
            Some(SVal::Str(v))
        }
        b'a' => {
            *pos += 2;
            let n: usize = read_until(pos, b':')?.parse().ok()?;
            *pos += 1; // {
            let mut v = Vec::new();
            for _ in 0..n {
                let k = parse_sval(s, pos)?;
                let x = parse_sval(s, pos)?;
                v.push((k, x));
            }
            *pos += 1; // }
            Some(SVal::Array(v))
        }
        _ => None,
    }
}

fn sval_num(v: &SVal) -> Num<'_> {
    match v {
        SVal::Int(i) => Num::Int(*i),
        SVal::Str(s) => Num::Str(s),
        SVal::Float(f) => Num::Float(*f),
        SVal::Null => Num::Null,
        SVal::Bool(b) => Num::Bool(*b),
        SVal::Array(_) => Num::Other("array"),
    }
}

/// `__unserialize()` as the engine runs it with [`object::unserialize_value`].
fn engine_unserialize_array(entries: &[(SVal, SVal)]) -> Result<Gmp, String> {
    let get = |k: i64| {
        entries
            .iter()
            .find(|(key, _)| matches!(key, SVal::Int(i) if *i == k))
            .map(|(_, v)| v)
    };
    let g = object::unserialize_value(get(0).map_or(Num::Null, sval_num)).map_err(err)?;
    if let Some(p) = get(1)
        && !matches!(p, SVal::Array(_))
    {
        return Err(format!("EException:{UNSERIALIZE_PROPERTIES_ERROR}"));
    }
    Ok(g)
}

fn engine_unserialize(s: &[u8]) -> String {
    if let Some(rest) = s.strip_prefix(b"O:3:\"GMP\":") {
        let colon = rest.iter().position(|&c| c == b':').unwrap();
        let n: usize = std::str::from_utf8(&rest[..colon])
            .unwrap()
            .parse()
            .unwrap();
        let mut pos = colon + 2;
        let mut entries = Vec::new();
        for _ in 0..n {
            let k = parse_sval(rest, &mut pos).unwrap();
            let v = parse_sval(rest, &mut pos).unwrap();
            entries.push((k, v));
        }
        return match engine_unserialize_array(&entries) {
            Ok(g) => enc(&Out::G(g)),
            Err(e) => e,
        };
    }
    if let Some(rest) = s.strip_prefix(b"C:3:\"GMP\":") {
        let colon = rest.iter().position(|&c| c == b':').unwrap();
        let payload = &rest[colon + 2..rest.len() - 1];
        let mut pos = 0;
        let Some(first) = parse_sval(payload, &mut pos) else {
            return format!("EException:{UNSERIALIZE_NUMBER_ERROR}");
        };
        let g = match object::unserialize_legacy_value(sval_num(&first)) {
            Ok(g) => g,
            Err(e) => return err(e),
        };
        return match parse_sval(payload, &mut pos) {
            Some(SVal::Array(_)) => enc(&Out::G(g)),
            _ => format!("EException:{UNSERIALIZE_PROPERTIES_ERROR}"),
        };
    }
    panic!("unexpected unserialize payload");
}

fn serialized(g: &Gmp) -> Vec<u8> {
    let h = object::serialize_value(g);
    format!("O:3:\"GMP\":1:{{i:0;s:{}:\"{}\";}}", h.len(), h).into_bytes()
}

// ------------------------------------------------------------------ calls

fn g(r: GmpResult<Gmp>) -> Result<Out, GmpError> {
    r.map(Out::G)
}

fn call(env: &mut Env, name: &str, args: &[Arg]) -> Result<Out, GmpError> {
    let n = |i: usize| args.get(i).map(Arg::num).unwrap_or(Num::Null);
    let int_or = |i: usize, d: i64| args.get(i).map_or(d, Arg::int);
    let pair = |r: GmpResult<(Gmp, Gmp)>| r.map(|(a, b)| Out::A(vec![Out::G(a), Out::G(b)]));
    Ok(match name {
        "gmp_init" => {
            // A Stringable object is passed as its string in coercive mode.
            let v = match (&args[0], env.strict) {
                (Arg::Stringable, false) => Num::Str(b"42"),
                (a, _) => a.num(),
            };
            return g(gmp_init(env, v, int_or(1, 0)));
        }
        "gmp_import" => {
            let Arg::Str(data) = &args[0] else { panic!() };
            return g(gmp_import(data, int_or(1, 1), int_or(2, 17)));
        }
        "gmp_export" => Out::S(gmp_export(env, n(0), int_or(1, 1), int_or(2, 17))?),
        "gmp_export_len" => Out::I(gmp_export(env, n(0), int_or(1, 1), 17)?.len() as i64),
        "gmp_intval" => Out::I(gmp_intval(env, n(0))?),
        "gmp_strval" => Out::S(gmp_strval(env, n(0), int_or(1, 10))?.into_bytes()),
        "gmp_add" => return g(gmp_add(env, n(0), n(1))),
        "gmp_sub" => return g(gmp_sub(env, n(0), n(1))),
        "gmp_mul" => return g(gmp_mul(env, n(0), n(1))),
        "gmp_div_qr" => return pair(gmp_div_qr(env, n(0), n(1), int_or(2, 0))),
        "gmp_div_q" => return g(gmp_div_q(env, n(0), n(1), int_or(2, 0))),
        "gmp_div" => return g(gmp_div(env, n(0), n(1), int_or(2, 0))),
        "gmp_div_r" => return g(gmp_div_r(env, n(0), n(1), int_or(2, 0))),
        "gmp_mod" => return g(gmp_mod(env, n(0), n(1))),
        "gmp_divexact" => return g(gmp_divexact(env, n(0), n(1))),
        "gmp_neg" => return g(gmp_neg(env, n(0))),
        "gmp_abs" => return g(gmp_abs(env, n(0))),
        "gmp_fact" => return g(gmp_fact(env, n(0))),
        "gmp_sqrt" => return g(gmp_sqrt(env, n(0))),
        "gmp_sqrtrem" => return pair(gmp_sqrtrem(env, n(0))),
        "gmp_root" => return g(gmp_root(env, n(0), int_or(1, 0))),
        "gmp_rootrem" => return pair(gmp_rootrem(env, n(0), int_or(1, 0))),
        "gmp_pow" => return g(gmp_pow(env, n(0), int_or(1, 0))),
        "gmp_powm" => return g(gmp_powm(env, n(0), n(1), n(2))),
        "gmp_perfect_square" => Out::B(gmp_perfect_square(env, n(0))?),
        "gmp_perfect_power" => Out::B(gmp_perfect_power(env, n(0))?),
        "gmp_prob_prime" => Out::I(gmp_prob_prime(env, n(0), int_or(1, 10))?),
        "gmp_gcd" => return g(gmp_gcd(env, n(0), n(1))),
        "gmp_gcdext" => {
            let r = gmp_gcdext(env, n(0), n(1))?;
            Out::A(vec![Out::G(r.g), Out::G(r.s), Out::G(r.t)])
        }
        "gmp_lcm" => return g(gmp_lcm(env, n(0), n(1))),
        "gmp_invert" => match gmp_invert(env, n(0), n(1))? {
            Some(v) => Out::G(v),
            None => Out::B(false),
        },
        "gmp_jacobi" => Out::I(gmp_jacobi(env, n(0), n(1))?),
        "gmp_legendre" => Out::I(gmp_legendre(env, n(0), n(1))?),
        "gmp_kronecker" => Out::I(gmp_kronecker(env, n(0), n(1))?),
        "gmp_cmp" => Out::I(gmp_cmp(env, n(0), n(1))?),
        "gmp_sign" => Out::I(gmp_sign(env, n(0))?),
        "gmp_random_seed" => {
            gmp_random_seed(env, n(0))?;
            Out::N
        }
        "gmp_random_bits" => return g(gmp_random_bits(int_or(0, 0))),
        "gmp_random_range" => return g(gmp_random_range(env, n(0), n(1))),
        "gmp_and" => return g(gmp_and(env, n(0), n(1))),
        "gmp_or" => return g(gmp_or(env, n(0), n(1))),
        "gmp_xor" => return g(gmp_xor(env, n(0), n(1))),
        "gmp_com" => return g(gmp_com(env, n(0))),
        "gmp_testbit" => Out::B(gmp_testbit(env, n(0), int_or(1, 0))?),
        "gmp_scan0" => Out::I(gmp_scan0(env, n(0), int_or(1, 0))?),
        "gmp_scan1" => Out::I(gmp_scan1(env, n(0), int_or(1, 0))?),
        "gmp_popcount" => Out::I(gmp_popcount(env, n(0))?),
        "gmp_hamdist" => Out::I(gmp_hamdist(env, n(0), n(1))?),
        "gmp_nextprime" => return g(gmp_nextprime(env, n(0))),
        "gmp_binomial" => return g(gmp_binomial(env, n(0), int_or(1, 0))),
        "setbit" | "clrbit" => {
            let func = if name == "setbit" {
                "gmp_setbit"
            } else {
                "gmp_clrbit"
            };
            let Arg::Gmp(v) = &args[0] else {
                return Err(gmp_object_type_error(func, args[0].type_name()));
            };
            let mut v = v.clone();
            if name == "setbit" {
                let Arg::Bool(b) = args[2] else { panic!() };
                gmp_setbit(&mut v, int_or(1, 0), b)?;
            } else {
                gmp_clrbit(&mut v, int_or(1, 0))?;
            }
            Out::G(v)
        }
        _ => panic!("unknown function {name}"),
    })
}

fn run_unary(ctx: &mut Diagnostics, name: &str, a: &Gmp) -> String {
    let out = match name {
        "-" => Out::G(object::negate(a)),
        "~" => Out::G(object::bitwise_not(a)),
        "+" => match object::binary_op(ctx, BinOp::Mul, Num::Gmp(a), Num::Int(1)) {
            Some(Ok(v)) => Out::G(v),
            _ => panic!(),
        },
        "++" => Out::G(object::increment(a)),
        "--" => Out::G(object::decrement(a)),
        "int" => Out::I(object::to_int(a)),
        "float" => Out::D(object::to_float(a)),
        "bool" => Out::B(object::to_bool(a)),
        "string" => Out::S(object::to_string(a).into_bytes()),
        "serialize" => Out::S(serialized(a)),
        "__serialize" => Out::A(vec![Out::S(object::serialize_value(a).into_bytes())]),
        _ => panic!("unary {name}"),
    };
    enc(&out)
}

fn notice_str(n: &goro_runtime::Notice) -> String {
    match n.level {
        Level::Deprecated => format!("D:{}", n.message),
        _ => format!("W:{}", n.message),
    }
}

#[test]
fn php85_fixture() {
    let text = include_str!("fixtures/gmp_php85.txt");
    let mut defs: HashMap<usize, Gmp> = HashMap::new();
    let mut failures = Vec::new();
    let mut count = 0;
    for line in text.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields[0] == "#def" {
            let v = parse_integer(fields[2].as_bytes(), 10).unwrap();
            defs.insert(fields[1].parse().unwrap(), Gmp(v));
            continue;
        }
        count += 1;
        let arrow = fields.iter().position(|&f| f == "=>").unwrap();
        let args: Vec<Arg> = fields[1..arrow].iter().map(|t| decode(t, &defs)).collect();
        let expected = fields[arrow + 1..].join("\t");
        let name = fields[0];
        let mut ctx = Diagnostics::default();
        let outcome = if let Some(op) = name.strip_prefix("op") {
            run_op(&mut ctx, op, &args[0], &args[1])
        } else if name == "unserialize" {
            let Arg::Str(s) = &args[0] else { panic!() };
            engine_unserialize(s)
        } else if let Some(rest) = name.strip_prefix("__unserialize") {
            let entries: Vec<(SVal, SVal)> = match rest {
                "_empty" => vec![],
                "_props_bad" => vec![
                    (SVal::Int(0), SVal::Str(b"5".to_vec())),
                    (SVal::Int(1), SVal::Int(3)),
                ],
                _ => {
                    let v = match &args[0] {
                        Arg::Str(s) => SVal::Str(s.clone()),
                        Arg::Int(i) => SVal::Int(*i),
                        Arg::Float(f) => SVal::Float(*f),
                        Arg::Bool(b) => SVal::Bool(*b),
                        Arg::Null => SVal::Null,
                        Arg::Array => SVal::Array(vec![]),
                        a => panic!("{a:?}"),
                    };
                    vec![(SVal::Int(0), v)]
                }
            };
            match engine_unserialize_array(&entries) {
                Ok(g) => enc(&Out::G(g)),
                Err(e) => e,
            }
        } else if let Some(u) = name.strip_prefix('u') {
            let Arg::Gmp(a) = &args[0] else { panic!() };
            run_unary(&mut ctx, u, a)
        } else {
            let (strict, fname) = match name.strip_prefix('!') {
                Some(f) => (true, f),
                None => (false, name),
            };
            let mut env = Env::new(&mut ctx, strict);
            match call(&mut env, fname, &args) {
                Ok(o) => enc(&o),
                Err(e) => err(e),
            }
        };
        let mut got: Vec<String> = ctx.take().iter().map(notice_str).collect();
        got.push(outcome);
        let got = got.join("\t");
        if got != expected {
            failures.push(format!("{line}\n    got: {got}"));
        }
    }
    let n = failures.len();
    let mut by_fn: HashMap<&str, usize> = HashMap::new();
    for f in &failures {
        *by_fn.entry(f.split('\t').next().unwrap()).or_default() += 1;
    }
    eprintln!("{by_fn:?}");
    let mut shown: HashMap<&str, usize> = HashMap::new();
    for f in &failures {
        let c = shown.entry(f.split('\t').next().unwrap()).or_default();
        *c += 1;
        if *c <= 4 {
            eprintln!("{f}");
        }
    }
    assert!(n == 0, "{n} of {count} cases differ");
}
