//! MD2 (RFC 1319), MD4 (RFC 1320) and MD5 (RFC 1321).
//!
//! The serialized layouts and buffer behaviours were observed on PHP 8.5
//! (see engine.rs): MD2 keeps its 48-byte working block, checksum, pending
//! bytes and a byte-wide pending count; MD4 keeps state, a 64-bit bit count
//! (low word first) and an RFC 1320-style buffer; MD5 keeps a byte count as
//! a 29-bit low word plus a high word, its state, buffer and sixteen unused
//! words.

use crate::engine::{BlockBuf, Engine, Field, Kind, Val, clone_box, f, le32};

// ---------------------------------------------------------------- MD2

/// The MD2 substitution table, from the digits of pi (RFC 1319, 3.2).
#[rustfmt::skip]
static MD2_S: [u8; 256] = [
    41, 46, 67, 201, 162, 216, 124, 1, 61, 54, 84, 161, 236, 240, 6, 19,
    98, 167, 5, 243, 192, 199, 115, 140, 152, 147, 43, 217, 188, 76, 130, 202,
    30, 155, 87, 60, 253, 212, 224, 22, 103, 66, 111, 24, 138, 23, 229, 18,
    190, 78, 196, 214, 218, 158, 222, 73, 160, 251, 245, 142, 187, 47, 238, 122,
    169, 104, 121, 145, 21, 178, 7, 63, 148, 194, 16, 137, 11, 34, 95, 33,
    128, 127, 93, 154, 90, 144, 50, 39, 53, 62, 204, 231, 191, 247, 151, 3,
    255, 25, 48, 179, 72, 165, 181, 209, 215, 94, 146, 42, 172, 86, 170, 198,
    79, 184, 56, 210, 150, 164, 125, 182, 118, 252, 107, 226, 156, 116, 4, 241,
    69, 157, 112, 89, 100, 113, 135, 32, 134, 91, 207, 101, 230, 45, 168, 2,
    27, 96, 37, 173, 174, 176, 185, 246, 28, 70, 97, 105, 52, 64, 126, 15,
    85, 71, 163, 35, 221, 81, 175, 58, 195, 92, 249, 206, 186, 197, 234, 38,
    44, 83, 13, 110, 133, 40, 132, 9, 211, 223, 205, 244, 65, 129, 77, 82,
    106, 220, 55, 200, 108, 193, 171, 250, 36, 225, 123, 8, 12, 189, 177, 74,
    120, 136, 149, 139, 227, 99, 232, 109, 233, 203, 213, 254, 59, 0, 29, 57,
    242, 239, 183, 14, 102, 88, 208, 228, 166, 119, 114, 248, 235, 117, 75, 10,
    49, 68, 80, 180, 143, 237, 31, 26, 219, 153, 141, 51, 159, 17, 131, 20,
];

pub(crate) const MD2_LAYOUT: &[Field] = &[
    f(Kind::Bytes(48), 1, 0),
    f(Kind::Bytes(16), 1, 48),
    f(Kind::Bytes(16), 1, 64),
    f(Kind::U8, 1, 80),
];

#[derive(Clone)]
pub(crate) struct Md2 {
    x: [u8; 48],
    c: [u8; 16],
    buf: [u8; 16],
    used: usize,
}

impl Md2 {
    pub(crate) fn new() -> Self {
        Md2 {
            x: [0; 48],
            c: [0; 16],
            buf: [0; 16],
            used: 0,
        }
    }

    fn block(&mut self, m: &[u8; 16]) {
        for j in 0..16 {
            self.x[16 + j] = m[j];
            self.x[32 + j] = self.x[16 + j] ^ self.x[j];
        }
        let mut t = 0u8;
        for i in 0..18u8 {
            for k in 0..48 {
                self.x[k] ^= MD2_S[t as usize];
                t = self.x[k];
            }
            t = t.wrapping_add(i);
        }
        // Checksum (RFC 1319 3.2 with its errata: C[j] is XORed).
        let mut l = self.c[15];
        for j in 0..16 {
            self.c[j] ^= MD2_S[(m[j] ^ l) as usize];
            l = self.c[j];
        }
    }
}

impl Engine for Md2 {
    fn update(&mut self, mut data: &[u8]) {
        if self.used != 0 {
            let take = 16 - self.used;
            if data.len() < take {
                self.buf[self.used..self.used + data.len()].copy_from_slice(data);
                self.used += data.len();
                return;
            }
            self.buf[self.used..].copy_from_slice(&data[..take]);
            let b = self.buf;
            self.block(&b);
            data = &data[take..];
            self.used = 0;
        }
        let (chunks, chunks_rest) = data.as_chunks::<16>();
        for b in chunks {
            self.block(b);
        }
        let rest = chunks_rest;
        self.buf[..rest.len()].copy_from_slice(rest);
        self.used = rest.len();
    }

    fn finish(&mut self, out: &mut [u8]) {
        let pad = 16 - self.used;
        let p = [pad as u8; 16];
        self.update(&p[..pad]);
        let c = self.c;
        self.block(&c);
        out.copy_from_slice(&self.x[..16]);
    }

    clone_box!();

    fn export(&self) -> Vec<Val> {
        vec![
            Val::Bytes(self.x.to_vec()),
            Val::Bytes(self.c.to_vec()),
            Val::Bytes(self.buf.to_vec()),
            Val::U8(self.used as u8),
        ]
    }

    fn import(&mut self, v: &[Val]) -> bool {
        let used = v[3].u32() as usize;
        if used >= 16 {
            return false;
        }
        self.x.copy_from_slice(v[0].bytes());
        self.c.copy_from_slice(v[1].bytes());
        self.buf.copy_from_slice(v[2].bytes());
        self.used = used;
        true
    }
}

// ---------------------------------------------------------------- MD4

pub(crate) const MD4_LAYOUT: &[Field] = &[
    f(Kind::U32, 4, 0),
    f(Kind::U32, 2, 16),
    f(Kind::Bytes(64), 1, 24),
];

#[derive(Clone)]
pub(crate) struct Md4 {
    h: [u32; 4],
    count: [u32; 2],
    b: BlockBuf<64>,
}

const MD_IV: [u32; 4] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476];

fn md4_compress(h: &mut [u32; 4], block: &[u8; 64]) {
    let mut x = [0u32; 16];
    for (i, w) in x.iter_mut().enumerate() {
        *w = le32(&block[i * 4..]);
    }
    let [mut a, mut b, mut c, mut d] = *h;
    macro_rules! r1 {
        ($a:ident, $b:ident, $c:ident, $d:ident, $k:expr, $s:expr) => {
            $a = $a
                .wrapping_add(($b & $c) | (!$b & $d))
                .wrapping_add(x[$k])
                .rotate_left($s);
        };
    }
    macro_rules! r2 {
        ($a:ident, $b:ident, $c:ident, $d:ident, $k:expr, $s:expr) => {
            $a = $a
                .wrapping_add(($b & $c) | ($b & $d) | ($c & $d))
                .wrapping_add(x[$k])
                .wrapping_add(0x5a827999)
                .rotate_left($s);
        };
    }
    macro_rules! r3 {
        ($a:ident, $b:ident, $c:ident, $d:ident, $k:expr, $s:expr) => {
            $a = $a
                .wrapping_add($b ^ $c ^ $d)
                .wrapping_add(x[$k])
                .wrapping_add(0x6ed9eba1)
                .rotate_left($s);
        };
    }
    for i in 0..4 {
        r1!(a, b, c, d, i * 4, 3);
        r1!(d, a, b, c, i * 4 + 1, 7);
        r1!(c, d, a, b, i * 4 + 2, 11);
        r1!(b, c, d, a, i * 4 + 3, 19);
    }
    for i in 0..4 {
        r2!(a, b, c, d, i, 3);
        r2!(d, a, b, c, i + 4, 5);
        r2!(c, d, a, b, i + 8, 9);
        r2!(b, c, d, a, i + 12, 13);
    }
    for i in [0, 2, 1, 3] {
        r3!(a, b, c, d, i, 3);
        r3!(d, a, b, c, i + 8, 9);
        r3!(c, d, a, b, i + 4, 11);
        r3!(b, c, d, a, i + 12, 15);
    }
    h[0] = h[0].wrapping_add(a);
    h[1] = h[1].wrapping_add(b);
    h[2] = h[2].wrapping_add(c);
    h[3] = h[3].wrapping_add(d);
}

impl Md4 {
    pub(crate) fn new() -> Self {
        Md4 {
            h: MD_IV,
            count: [0; 2],
            b: BlockBuf::new(),
        }
    }
}

/// Adds `len` bytes to a 64-bit bit counter held as two 32-bit words, low
/// word first (RFC 1320/1321 reference scheme). Returns the pending byte
/// count before the addition.
#[inline]
pub(crate) fn add_bits(count: &mut [u32; 2], len: usize, block: usize) -> usize {
    let used = ((count[0] >> 3) as usize) % block;
    let bits = (len as u64).wrapping_mul(8);
    let lo = count[0].wrapping_add(bits as u32);
    if lo < count[0] {
        count[1] = count[1].wrapping_add(1);
    }
    count[0] = lo;
    count[1] = count[1].wrapping_add(((len as u64) >> 29) as u32);
    used
}

impl Engine for Md4 {
    fn update(&mut self, data: &[u8]) {
        let used = add_bits(&mut self.count, data.len(), 64);
        let h = &mut self.h;
        self.b.absorb(used, data, true, |blks| {
            blks.iter().for_each(|b| md4_compress(h, b))
        });
    }

    fn finish(&mut self, out: &mut [u8]) {
        let bits = ((self.count[1] as u64) << 32) | self.count[0] as u64;
        let used = (self.count[0] >> 3) as usize % 64;
        let pad = if used < 56 { 56 - used } else { 120 - used };
        let mut p = [0u8; 72];
        p[0] = 0x80;
        p[pad..pad + 8].copy_from_slice(&bits.to_le_bytes());
        self.update(&p[..pad + 8]);
        for (i, w) in self.h.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
    }

    clone_box!();

    fn export(&self) -> Vec<Val> {
        let mut v: Vec<Val> = self.h.iter().map(|&w| Val::U32(w)).collect();
        v.push(Val::U32(self.count[0]));
        v.push(Val::U32(self.count[1]));
        v.push(Val::Bytes(self.b.buf.to_vec()));
        v
    }

    fn import(&mut self, v: &[Val]) -> bool {
        for i in 0..4 {
            self.h[i] = v[i].u32();
        }
        self.count = [v[4].u32(), v[5].u32()];
        self.b.buf.copy_from_slice(v[6].bytes());
        true
    }
}

// ---------------------------------------------------------------- MD5

pub(crate) const MD5_LAYOUT: &[Field] = &[
    f(Kind::U32, 2, 0),
    f(Kind::U32, 4, 8),
    f(Kind::Bytes(64), 1, 24),
    f(Kind::U32, 16, 88),
];

/// floor(abs(sin(i + 1)) * 2^32), RFC 1321 3.4.
#[rustfmt::skip]
const MD5_T: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];

#[inline(always)]
pub(crate) fn md5_compress(h: &mut [u32; 4], block: &[u8; 64]) {
    let (words, _) = block.as_chunks::<4>();
    let mut x = [0u32; 16];
    for (w, b) in x.iter_mut().zip(words) {
        *w = u32::from_le_bytes(*b);
    }
    let [mut a, mut b, mut c, mut d] = *h;
    // The message word and constant are added first, off the critical
    // path through the previous step's result; reading the constants
    // through an opaque reference keeps the compiler from moving the
    // constant additions after the round function.
    let t: &[u32; 64] = std::hint::black_box(&MD5_T);
    macro_rules! step {
        (F, $a:ident, $b:ident, $c:ident, $d:ident, $k:expr, $s:expr, $i:expr) => {
            $a = $a.wrapping_add(x[$k].wrapping_add(t[$i]));
            $a = $a.wrapping_add($d ^ ($b & ($c ^ $d)));
            $a = $a.rotate_left($s).wrapping_add($b);
        };
        (G, $a:ident, $b:ident, $c:ident, $d:ident, $k:expr, $s:expr, $i:expr) => {
            $a = $a.wrapping_add(x[$k].wrapping_add(t[$i]));
            $a = $a.wrapping_add(!$d & $c);
            $a = $a.wrapping_add($d & $b);
            $a = $a.rotate_left($s).wrapping_add($b);
        };
        (H, $a:ident, $b:ident, $c:ident, $d:ident, $k:expr, $s:expr, $i:expr) => {
            $a = $a.wrapping_add(x[$k].wrapping_add(t[$i]));
            $a = $a.wrapping_add($b ^ ($c ^ $d));
            $a = $a.rotate_left($s).wrapping_add($b);
        };
        (I, $a:ident, $b:ident, $c:ident, $d:ident, $k:expr, $s:expr, $i:expr) => {
            $a = $a.wrapping_add(x[$k].wrapping_add(t[$i]));
            $a = $a.wrapping_add($c ^ ($b | !$d));
            $a = $a.rotate_left($s).wrapping_add($b);
        };
    }
    macro_rules! round {
        ($f:ident, $base:expr, $k:expr, $s:expr) => {
            for r in 0..4 {
                let i = $base + r * 4;
                step!($f, a, b, c, d, $k(i), $s[0], i);
                step!($f, d, a, b, c, $k(i + 1), $s[1], i + 1);
                step!($f, c, d, a, b, $k(i + 2), $s[2], i + 2);
                step!($f, b, c, d, a, $k(i + 3), $s[3], i + 3);
            }
        };
    }
    round!(F, 0, |i: usize| i, [7, 12, 17, 22]);
    round!(G, 16, |i: usize| (5 * i + 1) % 16, [5, 9, 14, 20]);
    round!(H, 32, |i: usize| (3 * i + 5) % 16, [4, 11, 16, 23]);
    round!(I, 48, |i: usize| (7 * i) % 16, [6, 10, 15, 21]);
    h[0] = h[0].wrapping_add(a);
    h[1] = h[1].wrapping_add(b);
    h[2] = h[2].wrapping_add(c);
    h[3] = h[3].wrapping_add(d);
}

#[derive(Clone)]
pub(crate) struct Md5 {
    lo: u32,
    hi: u32,
    h: [u32; 4],
    b: BlockBuf<64>,
    spare: [u32; 16],
}

impl Md5 {
    pub(crate) fn new() -> Self {
        Md5 {
            lo: 0,
            hi: 0,
            h: MD_IV,
            b: BlockBuf::new(),
            spare: [0; 16],
        }
    }
}

impl Engine for Md5 {
    fn update(&mut self, data: &[u8]) {
        let saved = self.lo;
        let len = data.len() as u64;
        self.lo = saved.wrapping_add(len as u32) & 0x1fff_ffff;
        if self.lo < saved {
            self.hi = self.hi.wrapping_add(1);
        }
        self.hi = self.hi.wrapping_add((len >> 29) as u32);
        let h = &mut self.h;
        self.b.absorb((saved & 0x3f) as usize, data, false, |blks| {
            blks.iter().for_each(|b| md5_compress(h, b))
        });
    }

    fn finish(&mut self, out: &mut [u8]) {
        let used = (self.lo & 0x3f) as usize;
        let lo_bits = self.lo << 3;
        let hi = self.hi;
        let pad = if used < 56 { 56 - used } else { 120 - used };
        let mut p = [0u8; 72];
        p[0] = 0x80;
        p[pad..pad + 4].copy_from_slice(&lo_bits.to_le_bytes());
        p[pad + 4..pad + 8].copy_from_slice(&hi.to_le_bytes());
        self.update(&p[..pad + 8]);
        for (i, w) in self.h.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
    }

    clone_box!();

    fn export(&self) -> Vec<Val> {
        let mut v = vec![Val::U32(self.lo), Val::U32(self.hi)];
        v.extend(self.h.iter().map(|&w| Val::U32(w)));
        v.push(Val::Bytes(self.b.buf.to_vec()));
        v.extend(self.spare.iter().map(|&w| Val::U32(w)));
        v
    }

    fn import(&mut self, v: &[Val]) -> bool {
        self.lo = v[0].u32();
        self.hi = v[1].u32();
        for i in 0..4 {
            self.h[i] = v[2 + i].u32();
        }
        self.b.buf.copy_from_slice(v[6].bytes());
        for i in 0..16 {
            self.spare[i] = v[7 + i].u32();
        }
        true
    }
}
