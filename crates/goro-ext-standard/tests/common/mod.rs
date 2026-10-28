//! Reader for the PHP 8.5 fixture files (see tests/fixtures/common.inc for
//! the line format).
#![allow(dead_code)]

use goro_ext_standard::error::{Diag, StdError};
use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq)]
pub enum V {
    S(Vec<u8>),
    I(i64),
    B(bool),
    N,
    F(String),
    A(Vec<(V, V)>),
    /// A generated input, named by the driver (see `label()` in common.inc).
    G(String),
}

impl V {
    pub fn s(&self) -> &[u8] {
        match self {
            V::S(s) => s,
            _ => panic!("not a string: {self:?}"),
        }
    }
    pub fn i(&self) -> i64 {
        match self {
            V::I(i) => *i,
            _ => panic!("not an int: {self:?}"),
        }
    }
    pub fn b(&self) -> bool {
        match self {
            V::B(b) => *b,
            _ => panic!("not a bool: {self:?}"),
        }
    }
}

pub fn s(b: &[u8]) -> V {
    V::S(b.to_vec())
}

pub fn arr_str(rows: Vec<(Vec<u8>, Vec<u8>)>) -> V {
    V::A(rows.into_iter().map(|(k, v)| (V::S(k), V::S(v))).collect())
}

pub struct Case {
    pub line: usize,
    pub func: String,
    pub args: Vec<V>,
    pub expect: String,
    pub diags: Vec<String>,
    /// The last `# ...` comment line before the case (driver context).
    pub ctx: String,
}

fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        write!(s, "{x:02x}").unwrap();
    }
    s
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn parse_val(s: &[u8], i: &mut usize) -> V {
    let c = s[*i];
    *i += 1;
    let take = |i: &mut usize, f: &dyn Fn(u8) -> bool| -> String {
        let st = *i;
        while *i < s.len() && f(s[*i]) {
            *i += 1;
        }
        String::from_utf8(s[st..*i].to_vec()).unwrap()
    };
    match c {
        b's' => V::S(unhex(&take(i, &|c| c.is_ascii_hexdigit()))),
        b'i' => V::I(
            take(i, &|c| c == b'-' || c.is_ascii_digit())
                .parse()
                .unwrap(),
        ),
        b'T' => V::B(true),
        b'F' => V::B(false),
        b'N' => V::N,
        b'f' => V::F(take(i, &|c| {
            c != b' ' && c != b',' && c != b'}' && c != b'='
        })),
        b'g' => V::G(take(i, &|c| {
            c != b' ' && c != b',' && c != b'}' && c != b'='
        })),
        b'a' => {
            let _n = take(i, &|c| c.is_ascii_digit());
            assert_eq!(s[*i], b'{');
            *i += 1;
            let mut v = Vec::new();
            while s[*i] != b'}' {
                let k = parse_val(s, i);
                assert_eq!(s[*i], b'=');
                *i += 1;
                let x = parse_val(s, i);
                v.push((k, x));
                if s[*i] == b',' {
                    *i += 1;
                }
            }
            *i += 1;
            V::A(v)
        }
        _ => panic!(
            "bad value at {}: {:?}",
            *i,
            String::from_utf8_lossy(&s[..s.len().min(*i + 20)])
        ),
    }
}

pub fn enc(v: &V) -> String {
    match v {
        V::S(b) => format!("s{}", hex(b)),
        V::I(i) => format!("i{i}"),
        V::B(b) => (if *b { "T" } else { "F" }).into(),
        V::N => "N".into(),
        V::F(f) => format!("f{f}"),
        V::G(g) => format!("g{g}"),
        V::A(a) => {
            let parts: Vec<String> = a
                .iter()
                .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
                .collect();
            format!("a{}{{{}}}", a.len(), parts.join(","))
        }
    }
}

/// Inputs the drivers name instead of printing (see `label()` in common.inc).
fn generate(name: &str) -> Vec<u8> {
    let (kind, range) = name
        .split_once(':')
        .unwrap_or_else(|| panic!("unknown input {name}"));
    let (a, b) = range.split_once('-').unwrap();
    let (a, b): (u32, u32) = (a.parse().unwrap(), b.parse().unwrap());
    let mut out = Vec::new();
    for c in a..b {
        match kind {
            "bytes" => out.push(c as u8),
            "u8" => {
                // Like the driver's u8(): any value, surrogates included.
                if c < 0x80 {
                    out.push(c as u8);
                } else if c < 0x800 {
                    out.extend_from_slice(&[0xC0 | (c >> 6) as u8, 0x80 | (c & 0x3F) as u8]);
                } else if c < 0x10000 {
                    out.extend_from_slice(&[
                        0xE0 | (c >> 12) as u8,
                        0x80 | ((c >> 6) & 0x3F) as u8,
                        0x80 | (c & 0x3F) as u8,
                    ]);
                } else {
                    out.extend_from_slice(&[
                        0xF0 | (c >> 18) as u8,
                        0x80 | ((c >> 12) & 0x3F) as u8,
                        0x80 | ((c >> 6) & 0x3F) as u8,
                        0x80 | (c & 0x3F) as u8,
                    ]);
                }
                if c % 16 == 15 {
                    out.push(b'\n');
                }
            }
            _ => panic!("unknown input {name}"),
        }
    }
    out
}

pub fn load(name: &str) -> Vec<Case> {
    let env = format!("GORO_FIXTURE_{}", name.to_ascii_uppercase());
    let path = std::env::var(&env)
        .unwrap_or_else(|_| format!("{}/tests/fixtures/{name}.txt", env!("CARGO_MANIFEST_DIR")));
    let text = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut cases = Vec::new();
    let mut ctx = String::new();
    let mut inputs: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();
    for (ln, line) in text.split(|&b| b == b'\n').enumerate() {
        if let Some(def) = line.strip_prefix(b"#input ") {
            let def = String::from_utf8(def.to_vec()).unwrap();
            let (name, hex) = def.split_once(' ').unwrap();
            inputs.insert(name.to_string(), unhex(hex));
            continue;
        }
        if line.starts_with(b"# ") {
            ctx = String::from_utf8_lossy(&line[2..]).into_owned();
        }
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let sp = line.iter().position(|&b| b == b' ').unwrap();
        let func = String::from_utf8(line[..sp].to_vec()).unwrap();
        let mut i = sp + 1;
        let mut args = Vec::new();
        while line[i] != b'=' {
            if line[i] == b' ' {
                i += 1;
                continue;
            }
            let v = match parse_val(line, &mut i) {
                V::G(name) => V::S(
                    inputs
                        .get(&name)
                        .cloned()
                        .unwrap_or_else(|| generate(&name)),
                ),
                v => v,
            };
            args.push(v);
            assert_eq!(line[i], b' ', "line {}", ln + 1);
            i += 1;
        }
        i += 2;
        let rest = String::from_utf8(line[i..].to_vec()).unwrap();
        let mut toks = rest.split(' ').map(String::from);
        let expect = toks.next().unwrap();
        cases.push(Case {
            line: ln + 1,
            func,
            args,
            expect,
            diags: toks.collect(),
            ctx: ctx.clone(),
        });
    }
    cases
}

pub enum Out {
    V(V),
    X(StdError),
}

impl From<V> for Out {
    fn from(v: V) -> Out {
        Out::V(v)
    }
}

fn enc_out(o: &Out) -> String {
    match o {
        Out::V(v) => enc(v),
        Out::X(e) => {
            let (c, m) = match e {
                StdError::ValueError(m) => ("ValueError", m),
                StdError::TypeError(m) => ("TypeError", m),
                StdError::ArgumentCountError(m) => ("ArgumentCountError", m),
                StdError::Fatal(m) => ("Fatal", m),
            };
            format!("X{c}:{}", hex(m.as_bytes()))
        }
    }
}

fn enc_diag(func: &str, d: &Diag) -> String {
    let (k, m) = match d {
        Diag::Warning(m) => ('W', m),
        Diag::Notice(m) => ('n', m),
        Diag::Deprecated(m) => ('D', m),
    };
    format!("{k}{}", hex(format!("{func}(): {m}").as_bytes()))
}

fn md5hex(s: &str) -> String {
    hex(&purecrypto::hash::md5(s.as_bytes()))
}

fn show(tok: &str) -> String {
    let t = if tok.len() > 600 { &tok[..600] } else { tok };
    let mut out = String::new();
    let b = t.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if (b[i] == b's' || b[i] == b'W' || b[i] == b'n' || b[i] == b'D')
            && (i == 0 || !b[i - 1].is_ascii_hexdigit())
        {
            let st = i + 1;
            let mut j = st;
            while j < b.len() && b[j].is_ascii_hexdigit() {
                j += 1;
            }
            let bytes = unhex(&t[st..j - (j - st) % 2]);
            if b[i] != b's' {
                out.push(b[i] as char);
                out.push(':');
            }
            out.push('"');
            for ch in bytes.utf8_chunks() {
                out.push_str(&ch.valid().escape_debug().to_string());
                for b in ch.invalid() {
                    write!(out, "\\x{b:02x}").unwrap();
                }
            }
            out.push('"');
            i = j;
        } else {
            out.push(b[i] as char);
            i += 1;
        }
    }
    out
}

/// Cuts two encoded values to a window around their first difference.
fn diff_window(e: &str, g: &str) -> (String, String) {
    let p = e.bytes().zip(g.bytes()).take_while(|(a, b)| a == b).count();
    if p < 200 || !e.starts_with('s') || !g.starts_with('s') {
        return (e.to_string(), g.to_string());
    }
    let st = ((p - 81) & !1) + 1;
    (format!("…s{}", &e[st..]), format!("…s{}", &g[st..]))
}

/// Runs every case through `f` and fails with a report of the mismatches;
/// the second-round probes in extra.txt are checked too.
pub fn check(name: &str, f: impl Fn(&Case) -> Option<(Out, Vec<Diag>)>) {
    check_file(name, &f, true);
    check_file("extra", &f, false);
}

fn check_file(name: &str, f: &impl Fn(&Case) -> Option<(Out, Vec<Diag>)>, need_cases: bool) {
    let cases = load(name);
    let mut bad = 0;
    let mut report = String::new();
    let mut ran = 0;
    for c in &cases {
        let Some((out, diags)) = f(c) else { continue };
        ran += 1;
        let got = enc_out(&out);
        let ok_val = if c.expect.starts_with('m') {
            md5hex(&got) == c.expect[1..]
        } else {
            got == c.expect
        };
        let got_d: Vec<String> = diags.iter().map(|d| enc_diag(&c.func, d)).collect();
        if !ok_val || got_d != c.diags {
            bad += 1;
            if bad
                <= std::env::var("GORO_FIXTURE_SHOW")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(40)
            {
                let args: Vec<String> = c.args.iter().map(|a| show(&enc(a))).collect();
                writeln!(report, "line {}: {}({})", c.line, c.func, args.join(", ")).unwrap();
                if !ok_val {
                    let (e, g) = diff_window(&c.expect, &got);
                    writeln!(report, "   expected {}\n   got      {}", show(&e), show(&g)).unwrap();
                }
                if got_d != c.diags {
                    let e: Vec<String> = c.diags.iter().map(|d| show(d)).collect();
                    let g: Vec<String> = got_d.iter().map(|d| show(d)).collect();
                    writeln!(report, "   diags expected {e:?}\n   diags got      {g:?}").unwrap();
                }
            }
        }
    }
    assert!(ran > 0 || !need_cases, "no cases ran");
    assert!(
        bad == 0,
        "{bad} of {ran} cases differ from PHP 8.5:\n{report}"
    );
}
