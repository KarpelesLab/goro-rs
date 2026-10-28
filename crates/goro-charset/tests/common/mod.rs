//! Helpers shared by the fixture tests: a gzip reader (RFC 1952/1951) for
//! the compressed fixtures, and a small JSON reader for the ops fixture.

#![allow(dead_code)]

/// Decompresses a gzip file (one member).
pub fn gunzip(data: &[u8]) -> Vec<u8> {
    assert!(data.len() > 18 && data[0] == 0x1F && data[1] == 0x8B && data[2] == 8);
    let flags = data[3];
    let mut p = 10;
    if flags & 4 != 0 {
        let xlen = u16::from_le_bytes([data[p], data[p + 1]]) as usize;
        p += 2 + xlen;
    }
    if flags & 8 != 0 {
        while data[p] != 0 {
            p += 1;
        }
        p += 1;
    }
    if flags & 16 != 0 {
        while data[p] != 0 {
            p += 1;
        }
        p += 1;
    }
    if flags & 2 != 0 {
        p += 2;
    }
    inflate(&data[p..])
}

struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    bit: u32,
    nbits: u32,
}

impl Bits<'_> {
    fn need(&mut self, n: u32) {
        while self.nbits < n {
            let b = *self.d.get(self.pos).unwrap_or(&0) as u32;
            self.pos += 1;
            self.bit |= b << self.nbits;
            self.nbits += 8;
        }
    }
    fn get(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        self.need(n);
        let v = self.bit & ((1u32 << n) - 1);
        self.bit >>= n;
        self.nbits -= n;
        v
    }
}

struct Huff {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huff {
    fn new(lengths: &[u8]) -> Huff {
        let mut counts = [0u16; 16];
        for &l in lengths {
            counts[l as usize] += 1;
        }
        counts[0] = 0;
        let mut offs = [0u16; 16];
        for i in 1..16 {
            offs[i] = offs[i - 1] + counts[i - 1];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (s, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[offs[l as usize] as usize] = s as u16;
                offs[l as usize] += 1;
            }
        }
        Huff { counts, symbols }
    }

    fn decode(&self, b: &mut Bits) -> u16 {
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;
        for len in 1..16 {
            code |= b.get(1) as i32;
            let count = self.counts[len] as i32;
            if code - count < first {
                return self.symbols[(index + (code - first)) as usize];
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        panic!("bad huffman code");
    }
}

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// Inflates a raw DEFLATE stream.
pub fn inflate(d: &[u8]) -> Vec<u8> {
    let mut b = Bits {
        d,
        pos: 0,
        bit: 0,
        nbits: 0,
    };
    let mut out = Vec::new();
    loop {
        let last = b.get(1);
        let ty = b.get(2);
        match ty {
            0 => {
                b.bit = 0;
                b.nbits = 0;
                let len = u16::from_le_bytes([d[b.pos], d[b.pos + 1]]) as usize;
                b.pos += 4;
                out.extend_from_slice(&d[b.pos..b.pos + len]);
                b.pos += len;
            }
            1 | 2 => {
                let (lit, dist) = if ty == 1 {
                    let mut l = [0u8; 288];
                    l[..144].fill(8);
                    l[144..256].fill(9);
                    l[256..280].fill(7);
                    l[280..].fill(8);
                    (Huff::new(&l), Huff::new(&[5u8; 30]))
                } else {
                    let hlit = b.get(5) as usize + 257;
                    let hdist = b.get(5) as usize + 1;
                    let hclen = b.get(4) as usize + 4;
                    const ORDER: [usize; 19] = [
                        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
                    ];
                    let mut cl = [0u8; 19];
                    for &o in ORDER.iter().take(hclen) {
                        cl[o] = b.get(3) as u8;
                    }
                    let ch = Huff::new(&cl);
                    let mut lens = Vec::with_capacity(hlit + hdist);
                    while lens.len() < hlit + hdist {
                        let sym = ch.decode(&mut b);
                        match sym {
                            0..=15 => lens.push(sym as u8),
                            16 => {
                                let prev = *lens.last().unwrap();
                                let n = 3 + b.get(2) as usize;
                                lens.resize(lens.len() + n, prev);
                            }
                            17 => {
                                let n = 3 + b.get(3) as usize;
                                lens.resize(lens.len() + n, 0);
                            }
                            _ => {
                                let n = 11 + b.get(7) as usize;
                                lens.resize(lens.len() + n, 0);
                            }
                        }
                    }
                    (Huff::new(&lens[..hlit]), Huff::new(&lens[hlit..]))
                };
                loop {
                    let sym = lit.decode(&mut b) as usize;
                    if sym < 256 {
                        out.push(sym as u8);
                    } else if sym == 256 {
                        break;
                    } else {
                        let i = sym - 257;
                        let len = LEN_BASE[i] as usize + b.get(LEN_EXTRA[i] as u32) as usize;
                        let di = dist.decode(&mut b) as usize;
                        let dd = DIST_BASE[di] as usize + b.get(DIST_EXTRA[di] as u32) as usize;
                        let start = out.len() - dd;
                        for k in 0..len {
                            out.push(out[start + k]);
                        }
                    }
                }
            }
            _ => panic!("bad block type"),
        }
        if last == 1 {
            break;
        }
    }
    out
}

/// A JSON value (enough for the fixtures).
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn parse(s: &str) -> Json {
        let mut p = Parser {
            b: s.as_bytes(),
            i: 0,
        };
        p.value()
    }

    pub fn get(&self, k: &str) -> Option<&Json> {
        match self {
            Json::Obj(v) => v.iter().find(|e| e.0 == k).map(|e| &e.1),
            _ => None,
        }
    }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn value(&mut self) -> Json {
        self.ws();
        match self.b[self.i] {
            b'n' => {
                self.i += 4;
                Json::Null
            }
            b't' => {
                self.i += 4;
                Json::Bool(true)
            }
            b'f' => {
                self.i += 5;
                Json::Bool(false)
            }
            b'"' => Json::Str(self.string()),
            b'[' => {
                self.i += 1;
                let mut v = Vec::new();
                loop {
                    self.ws();
                    if self.b[self.i] == b']' {
                        self.i += 1;
                        break;
                    }
                    v.push(self.value());
                    self.ws();
                    if self.b[self.i] == b',' {
                        self.i += 1;
                    }
                }
                Json::Arr(v)
            }
            b'{' => {
                self.i += 1;
                let mut v = Vec::new();
                loop {
                    self.ws();
                    if self.b[self.i] == b'}' {
                        self.i += 1;
                        break;
                    }
                    let k = self.string();
                    self.ws();
                    self.i += 1; // ':'
                    let val = self.value();
                    v.push((k, val));
                    self.ws();
                    if self.b[self.i] == b',' {
                        self.i += 1;
                    }
                }
                Json::Obj(v)
            }
            _ => {
                let start = self.i;
                while self.i < self.b.len()
                    && matches!(
                        self.b[self.i],
                        b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
                    )
                {
                    self.i += 1;
                }
                Json::Num(
                    std::str::from_utf8(&self.b[start..self.i])
                        .unwrap()
                        .parse()
                        .unwrap(),
                )
            }
        }
    }

    fn string(&mut self) -> String {
        self.i += 1;
        let mut s = String::new();
        loop {
            let c = self.b[self.i];
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let e = self.b[self.i];
                    self.i += 1;
                    match e {
                        b'n' => s.push('\n'),
                        b't' => s.push('\t'),
                        b'r' => s.push('\r'),
                        b'b' => s.push('\u{8}'),
                        b'f' => s.push('\u{c}'),
                        b'u' => {
                            let h = std::str::from_utf8(&self.b[self.i..self.i + 4]).unwrap();
                            self.i += 4;
                            let mut cp = u32::from_str_radix(h, 16).unwrap();
                            if (0xD800..0xDC00).contains(&cp) {
                                let h2 =
                                    std::str::from_utf8(&self.b[self.i + 2..self.i + 6]).unwrap();
                                self.i += 6;
                                let lo = u32::from_str_radix(h2, 16).unwrap();
                                cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                            }
                            s.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                        }
                        other => s.push(other as char),
                    }
                }
                _ => {
                    // Copy a run of UTF-8 bytes.
                    let start = self.i - 1;
                    let mut end = self.i;
                    while end < self.b.len() && self.b[end] != b'"' && self.b[end] != b'\\' {
                        end += 1;
                    }
                    s.push_str(std::str::from_utf8(&self.b[start..end]).unwrap());
                    self.i = end;
                }
            }
        }
        s
    }
}
