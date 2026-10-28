//! Checks every line of tests/fixtures/hash_php85.txt, the output of
//! tests/fixtures/hash_driver.php on PHP 8.5.

use goro_ext_hash::{
    Diag, HashContext, HashOptions, Seed, SerInput, SerVal, StateSlot, hash, hash_hkdf, hash_hmac,
    hash_pbkdf2, lookup, to_hex,
};

fn prng(seed: u64, n: usize) -> Vec<u8> {
    let mut x = seed & 0x7fff_ffff;
    (0..n)
        .map(|_| {
            x = (x.wrapping_mul(1103515245).wrapping_add(12345)) & 0x7fff_ffff;
            ((x >> 16) & 255) as u8
        })
        .collect()
}

const SEQS: [&str; 13] = [
    "3,5",
    "60,10",
    "100,30,70",
    "1,63,1",
    "200,200",
    "130,500,7",
    "33,1,31,64,2",
    "64,64",
    "16,16,16",
    "7,7,7,7,7,7,7,7,7,7",
    "250,1",
    "1,250",
    "1100,3,1100",
];

fn input(id: &str) -> Vec<u8> {
    match id {
        "abc" => b"abc".to_vec(),
        "fox" => b"The quick brown fox jumps over the lazy dog".to_vec(),
        _ => {
            let n: usize = id[1..].parse().unwrap();
            prng(n as u64, n)
        }
    }
}

fn chunks(seq: &str) -> Vec<Vec<u8>> {
    match SEQS.iter().position(|s| *s == seq) {
        Some(si) => seq
            .split(',')
            .enumerate()
            .map(|(k, n)| prng((1000 + si * 10 + k) as u64, n.parse().unwrap()))
            .collect(),
        None => seq
            .split(',')
            .enumerate()
            .map(|(k, n)| prng((2000 + k) as u64, n.parse().unwrap()))
            .collect(),
    }
}

fn options(id: &str) -> HashOptions {
    let seed = |v| HashOptions {
        seed: Seed::Int(v),
        secret: None,
    };
    match id {
        "def" => HashOptions::default(),
        "seed0" => seed(0),
        "seed1" => seed(1),
        "seed9" => seed(9),
        "seed42" => seed(42),
        "seedbig" => seed((1 << 32) + 5),
        "seedneg" => seed(-1),
        "seedmax" => seed(i64::MAX),
        "sec136" => HashOptions {
            seed: Seed::Absent,
            secret: Some(prng(11, 136)),
        },
        "sec192" => HashOptions {
            seed: Seed::Absent,
            secret: Some(prng(12, 192)),
        },
        "sec200" => HashOptions {
            seed: Seed::Absent,
            secret: Some(prng(13, 200)),
        },
        "sec256" => HashOptions {
            seed: Seed::Absent,
            secret: Some(prng(14, 256)),
        },
        _ => panic!("unknown option set {id}"),
    }
}

fn b64decode(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => panic!("bad base64"),
    } as u32;
    let s = s.trim_end_matches('=').as_bytes();
    let mut out = Vec::new();
    for c in s.chunks(4) {
        let mut n = 0u32;
        for (i, &x) in c.iter().enumerate() {
            n |= val(x) << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if c.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if c.len() > 3 {
            out.push(n as u8);
        }
    }
    out
}

/// PHP's serialize() of a HashContext with no properties.
fn php_serialize(ctx: &HashContext) -> Vec<u8> {
    let s = ctx.serialize().unwrap();
    let mut o = Vec::new();
    o.extend_from_slice(b"O:11:\"HashContext\":5:{i:0;");
    o.extend_from_slice(format!("s:{}:\"{}\";", s.algo.len(), s.algo).as_bytes());
    o.extend_from_slice(format!("i:1;i:{};i:2;a:{}:{{", s.flags, s.state.len()).as_bytes());
    for (i, v) in s.state.iter().enumerate() {
        o.extend_from_slice(format!("i:{i};").as_bytes());
        match v {
            SerVal::Int(n) => o.extend_from_slice(format!("i:{n};").as_bytes()),
            SerVal::Str(b) => {
                o.extend_from_slice(format!("s:{}:\"", b.len()).as_bytes());
                o.extend_from_slice(b);
                o.extend_from_slice(b"\";");
            }
        }
    }
    o.extend_from_slice(format!("}}i:3;i:{};i:4;a:0:{{}}}}", s.magic).as_bytes());
    o
}

/// Parses the state array (element 2) out of a serialized HashContext.
fn php_state(ser: &[u8]) -> (String, Vec<SerVal>) {
    struct P<'a> {
        s: &'a [u8],
        p: usize,
    }
    impl P<'_> {
        fn lit(&mut self, t: &[u8]) {
            assert_eq!(&self.s[self.p..self.p + t.len()], t, "at {}", self.p);
            self.p += t.len();
        }
        fn num(&mut self, end: u8) -> i64 {
            let start = self.p;
            while self.s[self.p] != end {
                self.p += 1;
            }
            let v = std::str::from_utf8(&self.s[start..self.p])
                .unwrap()
                .parse()
                .unwrap();
            self.p += 1;
            v
        }
        fn string(&mut self) -> Vec<u8> {
            self.lit(b"s:");
            let n = self.num(b':') as usize;
            self.lit(b"\"");
            let v = self.s[self.p..self.p + n].to_vec();
            self.p += n;
            self.lit(b"\";");
            v
        }
    }
    let mut p = P { s: ser, p: 0 };
    p.lit(b"O:11:\"HashContext\":5:{i:0;");
    let algo = String::from_utf8(p.string()).unwrap();
    p.lit(b"i:1;i:");
    p.num(b';');
    p.lit(b"i:2;a:");
    let count = p.num(b':') as usize;
    p.lit(b"{");
    let mut vals = Vec::new();
    for k in 0..count {
        p.lit(format!("i:{k};").as_bytes());
        if p.s[p.p] == b'i' {
            p.lit(b"i:");
            vals.push(SerVal::Int(p.num(b';')));
        } else {
            vals.push(SerVal::Str(p.string()));
        }
    }
    (algo, vals)
}

fn b64encode(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut o = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                o.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                o.push('=');
            }
        }
    }
    o
}

fn hex(b: &[u8]) -> String {
    String::from_utf8(to_hex(b)).unwrap()
}

#[test]
fn php85_fixture() {
    let data = include_str!("fixtures/hash_php85.txt");
    let mut failures = Vec::new();
    let mut checked = 0;
    let mut fail = |line: &str, got: String| {
        failures.push((line.to_string(), got));
    };
    for line in data.lines() {
        let f: Vec<&str> = line.split(' ').collect();
        let mut diags: Vec<Diag> = Vec::new();
        checked += 1;
        match f[0] {
            "D" => {
                let got = hex(&hash(
                    f[1].as_bytes(),
                    &input(f[2]),
                    &HashOptions::default(),
                    &mut diags,
                )
                .unwrap());
                if got != f[3] {
                    fail(line, got);
                }
            }
            "C" => {
                let mut c = HashContext::new(
                    f[1].as_bytes(),
                    0,
                    None,
                    &HashOptions::default(),
                    &mut diags,
                )
                .unwrap();
                for ch in chunks(f[2]) {
                    c.update(&ch);
                }
                if f[4] != "-" {
                    let want = b64decode(f[4]);
                    let got = php_serialize(&c);
                    if got != want {
                        fail(line, format!("serialization {}", b64encode(&got)));
                    }
                    // Unserialize PHP's state and finish it.
                    let (algo, vals) = php_state(&want);
                    let mut u = HashContext::unserialize(
                        Some(algo.as_bytes()),
                        Some(0),
                        StateSlot::Array(&vals),
                        Some(lookup(f[1].as_bytes()).unwrap().magic()),
                        true,
                    )
                    .unwrap();
                    let got = hex(&u.finalize().unwrap());
                    if got != f[3] {
                        fail(line, format!("after unserialize {got}"));
                    }
                }
                let got = hex(&c.finalize().unwrap());
                if got != f[3] {
                    fail(line, got);
                }
            }
            "H" => {
                let key = match f[2] {
                    "empty" => vec![],
                    "k" => b"k".to_vec(),
                    "key16" => vec![b'K'; 16],
                    "key64" => prng(7, 64),
                    "key65" => prng(8, 65),
                    "key200" => prng(9, 200),
                    _ => unreachable!(),
                };
                let got = hex(&hash_hmac(f[1].as_bytes(), &input(f[3]), &key).unwrap());
                if got != f[4] {
                    fail(line, got);
                }
            }
            "K" => {
                let opt = |s: &str| {
                    if s == "-" {
                        Vec::new()
                    } else {
                        s.as_bytes().to_vec()
                    }
                };
                let got = hex(&hash_hkdf(
                    f[1].as_bytes(),
                    b"input key material",
                    f[2].parse().unwrap(),
                    &opt(f[3]),
                    &opt(f[4]),
                )
                .unwrap());
                if got != f[5] {
                    fail(line, got);
                }
            }
            "P" => {
                let bin = f[4] == "1";
                let out = hash_pbkdf2(
                    f[1].as_bytes(),
                    b"password",
                    b"salt",
                    f[2].parse().unwrap(),
                    f[3].parse().unwrap(),
                    bin,
                )
                .unwrap();
                let got = if bin {
                    hex(&out)
                } else {
                    String::from_utf8(out).unwrap()
                };
                if got != f[5] {
                    fail(line, got);
                }
            }
            "O" => {
                let got =
                    hex(&hash(f[1].as_bytes(), &input(f[3]), &options(f[2]), &mut diags).unwrap());
                if got != f[4] {
                    fail(line, got);
                }
            }
            "Q" => {
                let mut c =
                    HashContext::new(f[1].as_bytes(), 0, None, &options(f[2]), &mut diags).unwrap();
                for ch in chunks(f[3]) {
                    c.update(&ch);
                }
                let got = hex(&c.finalize().unwrap());
                if got != f[4] {
                    fail(line, got);
                }
            }
            "E" => checked -= 1,
            _ => panic!("bad line {line}"),
        }
    }
    let n = failures.len();
    if let Ok(path) = std::env::var("HASH_FIXTURE_DUMP") {
        let mut out = String::new();
        for (l, g) in &failures {
            out.push_str(&format!("{l}\t{g}\n"));
        }
        std::fs::write(path, out).unwrap();
    }
    let mut by_algo = std::collections::BTreeMap::<String, usize>::new();
    for (line, _) in &failures {
        let mut it = line.split(' ');
        let k = format!("{} {}", it.next().unwrap(), it.next().unwrap());
        *by_algo.entry(k).or_default() += 1;
    }
    let summary: Vec<String> = by_algo.iter().map(|(k, v)| format!("{k}: {v}")).collect();
    let detail: Vec<String> = failures
        .iter()
        .take(40)
        .map(|(l, g)| {
            format!(
                "{}\n   got {}",
                &l[..l.len().min(400)],
                &g[..g.len().min(400)]
            )
        })
        .collect();
    assert!(
        n == 0,
        "{n} of {checked} fixture lines differ:\n{}\n{}",
        summary.join("\n"),
        detail.join("\n")
    );
    let _ = SerInput::Other;
}
