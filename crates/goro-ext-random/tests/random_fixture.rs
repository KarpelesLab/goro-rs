//! Checks engines, Randomizer methods and the legacy functions against
//! `fixtures/random.txt`, the output of `fixtures/random.php` on PHP 8.5.

use goro_ext_random::legacy::LegacyRandom;
use goro_ext_random::randomizer as rz;
use goro_ext_random::{
    Engine, Generated, IntervalBoundary, Mt19937, MtMode, NativeEngine, PcgOneseq128XslRr64,
    RandomError, Xoshiro256StarStar,
};

/// `LenEngine` of the driver: lens[i % n] bytes per call cut from a
/// Xoshiro256** stream.
struct LenEngine {
    x: Xoshiro256StarStar,
    lens: Vec<usize>,
    i: usize,
    calls: usize,
}

impl Engine for LenEngine {
    fn generate(&mut self) -> Result<Generated, RandomError> {
        self.calls += 1;
        let l = self.lens[self.i % self.lens.len()];
        self.i += 1;
        let mut s = Vec::new();
        while s.len() < l {
            s.extend_from_slice(&self.x.next_u64().to_le_bytes());
        }
        Generated::from_bytes(&s[..l])
    }
}

/// `MtUser` of the driver: a user engine forwarding to Mt19937.
struct MtUser {
    m: Box<Mt19937>,
    calls: usize,
}

impl Engine for MtUser {
    fn generate(&mut self) -> Result<Generated, RandomError> {
        self.calls += 1;
        Generated::from_bytes(&self.m.generate()?.bytes())
    }
}

enum TestEngine {
    Native(NativeEngine),
    Len(LenEngine),
    MtU(MtUser),
}

impl Engine for TestEngine {
    fn generate(&mut self) -> Result<Generated, RandomError> {
        match self {
            TestEngine::Native(e) => e.generate(),
            TestEngine::Len(e) => e.generate(),
            TestEngine::MtU(e) => e.generate(),
        }
    }
    fn mt_php_scaling(&self) -> bool {
        match self {
            TestEngine::Native(e) => e.mt_php_scaling(),
            _ => false,
        }
    }
    fn as_mt19937(&mut self) -> Option<&mut Mt19937> {
        match self {
            TestEngine::Native(e) => e.as_mt19937(),
            _ => None,
        }
    }
}

impl TestEngine {
    fn after(&mut self) -> String {
        match self {
            TestEngine::Native(e) => hex(&e.generate().unwrap().bytes()),
            TestEngine::Len(e) => e.calls.to_string(),
            TestEngine::MtU(e) => e.calls.to_string(),
        }
    }
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|b| format!("{b:02x}")).collect()
}

fn fh(f: f64) -> String {
    format!("{:016x}", f.to_bits())
}

fn src(n: usize) -> Vec<u8> {
    (0..n).map(|i| ((i * 37 + 11) % 256) as u8).collect()
}

fn engine(spec: &str) -> TestEngine {
    let p: Vec<&str> = spec.split(' ').collect();
    let int = |s: &str| s.parse::<i64>().unwrap();
    TestEngine::Native(match p[0] {
        "mt" => NativeEngine::Mt19937(Box::new(Mt19937::new(
            int(p[1]),
            MtMode::from_int(int(p[2])),
        ))),
        "pcg" => NativeEngine::Pcg(PcgOneseq128XslRr64::from_int(int(p[1]))),
        "pcgs" => NativeEngine::Pcg(PcgOneseq128XslRr64::from_string(&unhex(p[1])).unwrap()),
        "xo" => NativeEngine::Xoshiro(Xoshiro256StarStar::from_int(int(p[1]))),
        "xos" => NativeEngine::Xoshiro(Xoshiro256StarStar::from_string(&unhex(p[1])).unwrap()),
        "user" => {
            return TestEngine::Len(LenEngine {
                x: Xoshiro256StarStar::from_int(int(p[1])),
                lens: p[2].split(',').map(|l| l.parse().unwrap()).collect(),
                i: 0,
                calls: 0,
            });
        }
        "mtu" => {
            return TestEngine::MtU(MtUser {
                m: Box::new(Mt19937::new(int(p[1]), MtMode::from_int(int(p[2])))),
                calls: 0,
            });
        }
        other => panic!("engine {other}"),
    })
}

fn err_text(e: RandomError) -> String {
    match e {
        RandomError::ValueError(m) => format!("E ValueError: {m}"),
        RandomError::BrokenEngine(m) => format!("E Random\\BrokenRandomEngineError: {m}"),
        RandomError::RandomException(m) => format!("E Random\\RandomException: {m}"),
        RandomError::Exception(m) => format!("E Exception: {m}"),
        RandomError::Propagate => "E propagate".into(),
    }
}

fn join<T: ToString>(v: impl IntoIterator<Item = T>) -> String {
    v.into_iter()
        .map(|x| x.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

fn run_op(e: &mut TestEngine, op: &str) -> Result<String, RandomError> {
    let p: Vec<&str> = op.split(' ').collect();
    let int = |i: usize| p[i].parse::<i64>().unwrap();
    let flt = |i: usize| f64::from_bits(u64::from_str_radix(p[i], 16).unwrap());
    Ok(match p[0] {
        "gen" => join((0..16).map(|_| hex(&e.generate().unwrap().bytes()))),
        "nextInt" => {
            let mut v = Vec::new();
            for _ in 0..8 {
                v.push(rz::next_int(e)?);
            }
            join(v)
        }
        "nextFloat" => {
            let mut v = Vec::new();
            for _ in 0..8 {
                v.push(fh(rz::next_float(e)?));
            }
            join(v)
        }
        "getInt" => {
            let mut v = Vec::new();
            for _ in 0..10 {
                v.push(rz::get_int(e, int(1), int(2))?);
            }
            join(v)
        }
        "getFloat" => {
            let b = match p[3] {
                "CC" => IntervalBoundary::ClosedClosed,
                "CO" => IntervalBoundary::ClosedOpen,
                "OC" => IntervalBoundary::OpenClosed,
                _ => IntervalBoundary::OpenOpen,
            };
            let mut v = Vec::new();
            for _ in 0..6 {
                v.push(fh(rz::get_float(e, flt(1), flt(2), b)?));
            }
            join(v)
        }
        "getBytes" => hex(&rz::get_bytes(e, int(1))?),
        "gbfs" => hex(&rz::get_bytes_from_string(
            e,
            &src(int(1) as usize),
            int(2),
        )?),
        "shuffleBytes" => {
            let mut s = src(int(1) as usize);
            rz::shuffle_bytes(e, &mut s)?;
            hex(&s)
        }
        "shuffleArray" => {
            let mut v: Vec<usize> = (0..int(1) as usize).collect();
            rz::shuffle_array(e, &mut v)?;
            join(v)
        }
        "pick" => join(rz::pick_array_keys(e, int(1) as usize, int(2))?),
        other => panic!("op {other}"),
    })
}

fn run_legacy(l: &mut LegacyRandom, op: &str) -> String {
    let p: Vec<&str> = op.split(' ').collect();
    let n = |i: usize| p[i].parse::<usize>().unwrap();
    match p[0] {
        "shuffle" => {
            let mut v: Vec<usize> = (0..n(1)).collect();
            l.shuffle(&mut v);
            join(v)
        }
        "str_shuffle" => {
            let mut s = src(n(1));
            l.str_shuffle(&mut s);
            hex(&s)
        }
        "array_rand" => join(l.array_rand(&n(1), n(2) as i64).unwrap()),
        "getrand" => format!(
            "{} {}",
            LegacyRandom::getrandmax(),
            LegacyRandom::getrandmax()
        ),
        other => panic!("legacy op {other}"),
    }
}

#[test]
fn random_fixture() {
    let data = include_str!("fixtures/random.txt");
    let mut failures: Vec<String> = Vec::new();
    let mut by_op: std::collections::BTreeMap<String, usize> = Default::default();
    let mut n = 0;
    for line in data.lines() {
        let mut parts = line.splitn(3, " | ");
        let (espec, op, rest) = (
            parts.next().unwrap(),
            parts.next().unwrap(),
            parts.next().unwrap(),
        );
        let (want, want_after) = rest.rsplit_once(" ; ").unwrap();
        let (got, got_after) = if let Some(l) = espec.strip_prefix("legacy ") {
            let p: Vec<i64> = l.split(' ').map(|x| x.parse().unwrap()).collect();
            let mut lr = LegacyRandom::new();
            lr.mt_srand(Some(p[0]), p[1]);
            let got = run_legacy(&mut lr, op);
            (got, lr.mt_rand().to_string())
        } else {
            let mut e = engine(espec);
            let got = run_op(&mut e, op).unwrap_or_else(err_text);
            (got, e.after())
        };
        n += 1;
        if got != want || got_after != want_after {
            let kind = op.split(' ').next().unwrap().to_string();
            *by_op.entry(kind).or_default() += 1;
            if failures.len() < 40 {
                failures.push(format!(
                    "{espec} | {op}\n  want {want} ; {want_after}\n  got  {got} ; {got_after}"
                ));
            }
        }
    }
    assert!(n > 6000, "{n} lines");
    assert!(failures.is_empty(), "{:?}\n{}", by_op, failures.join("\n"));
}
