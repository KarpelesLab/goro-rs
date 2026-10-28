//! Shared helpers for the PHP 8.5 fixture tests: the deterministic inputs
//! of tests/fixtures/gen_data.php and the fixture line format.
#![allow(dead_code)]

use std::collections::BTreeMap;

pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed & 0x7fff_ffff)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = (self.0 * 1103515245 + 12345) & 0x7fff_ffff;
        self.0 >> 16
    }
}

pub fn lcg_bytes(seed: u64, n: usize) -> Vec<u8> {
    let mut x = seed & 0x7fff_ffff;
    (0..n)
        .map(|_| {
            x = (x * 1103515245 + 12345) & 0x7fff_ffff;
            ((x >> 16) & 0xff) as u8
        })
        .collect()
}

const WORDS: [&str; 60] = [
    "the",
    "of",
    "and",
    "a",
    "to",
    "in",
    "is",
    "you",
    "that",
    "it",
    "he",
    "was",
    "for",
    "on",
    "are",
    "as",
    "with",
    "his",
    "they",
    "I",
    "at",
    "be",
    "this",
    "have",
    "from",
    "or",
    "one",
    "had",
    "by",
    "word",
    "but",
    "not",
    "what",
    "all",
    "were",
    "we",
    "when",
    "your",
    "can",
    "said",
    "there",
    "use",
    "an",
    "each",
    "which",
    "she",
    "do",
    "how",
    "their",
    "if",
    "compression",
    "deflate",
    "window",
    "huffman",
    "literal",
    "distance",
    "PHP",
    "zlib",
    "stream",
    "buffer",
];

pub fn text_data(seed: u64, n: usize) -> Vec<u8> {
    let mut r = Lcg::new(seed);
    let mut out = Vec::new();
    while out.len() < n {
        let w = WORDS[(r.next() % WORDS.len() as u64) as usize];
        let k = r.next() % 16;
        let sep = match k {
            0 => ".\n",
            1 => ", ",
            _ => " ",
        };
        out.extend_from_slice(w.as_bytes());
        out.extend_from_slice(sep.as_bytes());
    }
    out.truncate(n);
    out
}

pub fn mixed_data(seed: u64, n: usize) -> Vec<u8> {
    let mut r = Lcg::new(seed);
    let mut out = Vec::new();
    while out.len() < n {
        let len = 1 + (r.next() % 3000) as usize;
        if r.next().is_multiple_of(3) {
            let s = r.next();
            out.extend(lcg_bytes(s, len));
        } else {
            let s = r.next();
            out.extend(text_data(s, len));
        }
    }
    out.truncate(n);
    out
}

pub fn runs_data(seed: u64, n: usize) -> Vec<u8> {
    let mut r = Lcg::new(seed);
    let mut out = Vec::new();
    while out.len() < n {
        let c = (r.next() & 0xff) as u8;
        let k = 1 + (r.next() % 600) as usize;
        out.extend(std::iter::repeat_n(c, k));
    }
    out.truncate(n);
    out
}

pub fn alpha_data(seed: u64, n: usize, alpha: &[u8]) -> Vec<u8> {
    let mut r = Lcg::new(seed);
    (0..n)
        .map(|_| alpha[(r.next() % alpha.len() as u64) as usize])
        .collect()
}

pub fn fib_data(seed: u64) -> Vec<u8> {
    let (mut a, mut b) = (1usize, 1usize);
    let mut s = Vec::new();
    for i in 0..25u8 {
        s.extend(std::iter::repeat_n(i + 65, a));
        (a, b) = (b, a + b);
    }
    let mut r = Lcg::new(seed);
    let mut i = s.len() - 1;
    while i > 0 {
        let hi = r.next();
        let lo = r.next();
        let j = (((hi << 15) | lo) % (i as u64 + 1)) as usize;
        s.swap(i, j);
        i -= 1;
    }
    s
}

pub fn numbers_data(n: usize) -> Vec<u8> {
    let mut s = Vec::new();
    for i in 0..n {
        s.extend(format!("{i} ").bytes());
    }
    s
}

pub fn struct_data(seed: u64, n: usize) -> Vec<u8> {
    let mut r = Lcg::new(seed);
    let mut out: Vec<u8> = Vec::new();
    while out.len() < n {
        let v = (r.next() % 1000) as u32;
        let l = (out.len() & 0xffff) as u16;
        out.extend(v.to_le_bytes());
        out.extend(l.to_le_bytes());
    }
    out.truncate(n);
    out
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

pub fn inputs() -> BTreeMap<&'static str, Vec<u8>> {
    let mut m = BTreeMap::new();
    let all: Vec<u8> = (0..=255u8).collect();
    let p1 = lcg_bytes(7, 32505);
    let p2 = lcg_bytes(8, 32507);
    let p3 = text_data(9, 32700);
    m.insert("empty", Vec::new());
    m.insert("a", b"a".to_vec());
    m.insert("hello", b"hello world".to_vec());
    m.insert("a15", vec![b'a'; 15]);
    m.insert("abc100", b"abc".repeat(100));
    m.insert("allbytes", all);
    m.insert("zeros100k", vec![0; 100000]);
    m.insert("rand1000", lcg_bytes(1, 1000));
    m.insert("rand70000", lcg_bytes(2, 70000));
    m.insert("text5000", text_data(3, 5000));
    m.insert("text300k", text_data(4, 300000));
    m.insert("mixed400k", mixed_data(5, 400000));
    m.insert("numbers", numbers_data(30000));
    m.insert("period32505", cat(&[&p1, &p1, &p1]));
    m.insert("period32507", cat(&[&p2, &p2, &p2, &p2[..1000]]));
    m.insert("period_text", cat(&[&p3, &p3, &p3[..20000]]));
    m.insert("runs200k", runs_data(10, 200000));
    m.insert("alpha4", alpha_data(11, 100000, b"ACGT"));
    m.insert("alpha2", alpha_data(12, 50000, b"ab"));
    m.insert(
        "textend",
        cat(&[&text_data(13, 100000), &b"abcabc".repeat(50)]),
    );
    m.insert("struct", struct_data(14, 80000));
    m.insert("x258", vec![b'x'; 258]);
    m.insert("x259", vec![b'x'; 259]);
    m.insert("x262", vec![b'x'; 262]);
    m.insert("x263", vec![b'x'; 263]);
    m.insert("x1000", vec![b'x'; 1000]);
    m.insert("fib", fib_data(15));
    m.insert("text70k", text_data(16, 70000));
    m
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

pub fn md5_hex(b: &[u8]) -> String {
    let mut h = goro_ext_hash::Hasher::plain(goro_ext_hash::lookup(b"md5").unwrap());
    h.update(b);
    hex(&h.finish())
}

/// The fixture representation of an output (gen_data.php's out_repr()).
pub fn out_repr(b: &[u8]) -> String {
    if b.len() <= 400 {
        hex(b)
    } else {
        format!("{}:{}", b.len(), md5_hex(b))
    }
}

/// Reads "case<TAB>value" lines.
pub fn fixture(name: &str) -> Vec<(String, String)> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{path}: {e}"))
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            let (k, v) = l.split_once('\t').unwrap_or((l, ""));
            (k.to_string(), v.to_string())
        })
        .collect()
}
