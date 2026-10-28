//! RIPEMD-128/160/256/320, from the designers' pseudo-code (Dobbertin,
//! Bosselaers, Preneel: homes.esat.kuleuven.be/~bosselae/ripemd160.html,
//! rmd128.txt, rmd160.txt, rmd256.txt, rmd320.txt).
//!
//! Serialized layout observed on PHP 8.5: state words, a 64-bit bit count
//! (low word first), then an RFC 1321-style 64-byte buffer.

use crate::engine::{BlockBuf, Engine, Field, Kind, Val, clone_box, f, le32};
use crate::md::add_bits;

pub(crate) const fn layout(words: usize) -> [Field; 3] {
    [
        f(Kind::U32, words, 0),
        f(Kind::U32, 2, words * 4),
        f(Kind::Bytes(64), 1, words * 4 + 8),
    ]
}
pub(crate) const RMD128_LAYOUT: [Field; 3] = layout(4);
pub(crate) const RMD160_LAYOUT: [Field; 3] = layout(5);
pub(crate) const RMD256_LAYOUT: [Field; 3] = layout(8);
pub(crate) const RMD320_LAYOUT: [Field; 3] = layout(10);

#[rustfmt::skip]
const R: [usize; 80] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
    7, 4, 13, 1, 10, 6, 15, 3, 12, 0, 9, 5, 2, 14, 11, 8,
    3, 10, 14, 4, 9, 15, 8, 1, 2, 7, 0, 6, 13, 11, 5, 12,
    1, 9, 11, 10, 0, 8, 12, 4, 13, 3, 7, 15, 14, 5, 6, 2,
    4, 0, 5, 9, 7, 12, 2, 10, 14, 1, 3, 8, 11, 6, 15, 13,
];
#[rustfmt::skip]
const RP: [usize; 80] = [
    5, 14, 7, 0, 9, 2, 11, 4, 13, 6, 15, 8, 1, 10, 3, 12,
    6, 11, 3, 7, 0, 13, 5, 10, 14, 15, 8, 12, 4, 9, 1, 2,
    15, 5, 1, 3, 7, 14, 6, 9, 11, 8, 12, 2, 10, 0, 4, 13,
    8, 6, 4, 1, 3, 11, 15, 0, 5, 12, 2, 13, 9, 7, 10, 14,
    12, 15, 10, 4, 1, 5, 8, 7, 6, 2, 13, 14, 0, 3, 9, 11,
];
#[rustfmt::skip]
const S: [u32; 80] = [
    11, 14, 15, 12, 5, 8, 7, 9, 11, 13, 14, 15, 6, 7, 9, 8,
    7, 6, 8, 13, 11, 9, 7, 15, 7, 12, 15, 9, 11, 7, 13, 12,
    11, 13, 6, 7, 14, 9, 13, 15, 14, 8, 13, 6, 5, 12, 7, 5,
    11, 12, 14, 15, 14, 15, 9, 8, 9, 14, 5, 6, 8, 6, 5, 12,
    9, 15, 5, 11, 6, 8, 13, 12, 5, 12, 13, 14, 11, 8, 5, 6,
];
#[rustfmt::skip]
const SP: [u32; 80] = [
    8, 9, 9, 11, 13, 15, 15, 5, 7, 7, 8, 11, 14, 14, 12, 6,
    9, 13, 15, 7, 12, 8, 9, 11, 7, 7, 12, 7, 6, 15, 13, 11,
    9, 7, 15, 11, 8, 6, 6, 14, 12, 13, 5, 14, 13, 13, 7, 5,
    15, 5, 8, 11, 14, 14, 6, 14, 6, 9, 12, 9, 12, 5, 15, 8,
    8, 5, 12, 9, 12, 5, 14, 6, 8, 13, 6, 5, 15, 13, 11, 11,
];
const K: [u32; 5] = [0x00000000, 0x5a827999, 0x6ed9eba1, 0x8f1bbcdc, 0xa953fd4e];
const KP160: [u32; 5] = [0x50a28be6, 0x5c4dd124, 0x6d703ef3, 0x7a6d76e9, 0x00000000];
const KP128: [u32; 4] = [0x50a28be6, 0x5c4dd124, 0x6d703ef3, 0x00000000];

#[inline(always)]
fn fun(j: usize, x: u32, y: u32, z: u32) -> u32 {
    match j / 16 {
        0 => x ^ y ^ z,
        1 => (x & y) | (!x & z),
        2 => (x | !y) ^ z,
        3 => (x & z) | (y & !z),
        _ => x ^ (y | !z),
    }
}

fn words(block: &[u8; 64]) -> [u32; 16] {
    let mut x = [0u32; 16];
    for (i, w) in x.iter_mut().enumerate() {
        *w = le32(&block[i * 4..]);
    }
    x
}

/// The two parallel lines of RIPEMD-128/256 (64 steps); `swap` exchanges
/// a chaining word after each round for RIPEMD-256.
fn lines128(l: &mut [u32; 4], r: &mut [u32; 4], x: &[u32; 16], swap: bool) {
    let [mut a, mut b, mut c, mut d] = *l;
    let [mut ap, mut bp, mut cp, mut dp] = *r;
    for j in 0..64 {
        let t = a
            .wrapping_add(fun(j, b, c, d))
            .wrapping_add(x[R[j]])
            .wrapping_add(K[j / 16])
            .rotate_left(S[j]);
        a = d;
        d = c;
        c = b;
        b = t;
        let t = ap
            .wrapping_add(fun(63 - j, bp, cp, dp))
            .wrapping_add(x[RP[j]])
            .wrapping_add(KP128[j / 16])
            .rotate_left(SP[j]);
        ap = dp;
        dp = cp;
        cp = bp;
        bp = t;
        if swap {
            match j {
                15 => std::mem::swap(&mut a, &mut ap),
                31 => std::mem::swap(&mut b, &mut bp),
                47 => std::mem::swap(&mut c, &mut cp),
                63 => std::mem::swap(&mut d, &mut dp),
                _ => {}
            }
        }
    }
    *l = [a, b, c, d];
    *r = [ap, bp, cp, dp];
}

/// The two parallel lines of RIPEMD-160/320 (80 steps).
fn lines160(l: &mut [u32; 5], r: &mut [u32; 5], x: &[u32; 16], swap: bool) {
    let [mut a, mut b, mut c, mut d, mut e] = *l;
    let [mut ap, mut bp, mut cp, mut dp, mut ep] = *r;
    for j in 0..80 {
        let t = a
            .wrapping_add(fun(j, b, c, d))
            .wrapping_add(x[R[j]])
            .wrapping_add(K[j / 16])
            .rotate_left(S[j])
            .wrapping_add(e);
        a = e;
        e = d;
        d = c.rotate_left(10);
        c = b;
        b = t;
        let t = ap
            .wrapping_add(fun(79 - j, bp, cp, dp))
            .wrapping_add(x[RP[j]])
            .wrapping_add(KP160[j / 16])
            .rotate_left(SP[j])
            .wrapping_add(ep);
        ap = ep;
        ep = dp;
        dp = cp.rotate_left(10);
        cp = bp;
        bp = t;
        if swap {
            match j {
                15 => std::mem::swap(&mut b, &mut bp),
                31 => std::mem::swap(&mut d, &mut dp),
                47 => std::mem::swap(&mut a, &mut ap),
                63 => std::mem::swap(&mut c, &mut cp),
                79 => std::mem::swap(&mut e, &mut ep),
                _ => {}
            }
        }
    }
    *l = [a, b, c, d, e];
    *r = [ap, bp, cp, dp, ep];
}

fn compress(h: &mut [u32; 10], words_n: usize, block: &[u8; 64]) {
    let x = words(block);
    match words_n {
        4 => {
            let mut l = [h[0], h[1], h[2], h[3]];
            let mut r = l;
            lines128(&mut l, &mut r, &x, false);
            let t = h[1].wrapping_add(l[2]).wrapping_add(r[3]);
            h[1] = h[2].wrapping_add(l[3]).wrapping_add(r[0]);
            h[2] = h[3].wrapping_add(l[0]).wrapping_add(r[1]);
            h[3] = h[0].wrapping_add(l[1]).wrapping_add(r[2]);
            h[0] = t;
        }
        5 => {
            let mut l = [h[0], h[1], h[2], h[3], h[4]];
            let mut r = l;
            lines160(&mut l, &mut r, &x, false);
            let t = h[1].wrapping_add(l[2]).wrapping_add(r[3]);
            h[1] = h[2].wrapping_add(l[3]).wrapping_add(r[4]);
            h[2] = h[3].wrapping_add(l[4]).wrapping_add(r[0]);
            h[3] = h[4].wrapping_add(l[0]).wrapping_add(r[1]);
            h[4] = h[0].wrapping_add(l[1]).wrapping_add(r[2]);
            h[0] = t;
        }
        8 => {
            let mut l = [h[0], h[1], h[2], h[3]];
            let mut r = [h[4], h[5], h[6], h[7]];
            lines128(&mut l, &mut r, &x, true);
            for i in 0..4 {
                h[i] = h[i].wrapping_add(l[i]);
                h[4 + i] = h[4 + i].wrapping_add(r[i]);
            }
        }
        _ => {
            let mut l = [h[0], h[1], h[2], h[3], h[4]];
            let mut r = [h[5], h[6], h[7], h[8], h[9]];
            lines160(&mut l, &mut r, &x, true);
            for i in 0..5 {
                h[i] = h[i].wrapping_add(l[i]);
                h[5 + i] = h[5 + i].wrapping_add(r[i]);
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct Ripemd {
    h: [u32; 10],
    n: usize,
    count: [u32; 2],
    b: BlockBuf<64>,
}

impl Ripemd {
    pub(crate) fn new(bits: usize) -> Self {
        let (n, iv): (usize, &[u32]) = match bits {
            128 => (4, &[0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476]),
            160 => (
                5,
                &[0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0],
            ),
            256 => (
                8,
                &[
                    0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0x76543210, 0xfedcba98,
                    0x89abcdef, 0x01234567,
                ],
            ),
            _ => (
                10,
                &[
                    0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0, 0x76543210,
                    0xfedcba98, 0x89abcdef, 0x01234567, 0x3c2d1e0f,
                ],
            ),
        };
        let mut h = [0u32; 10];
        h[..n].copy_from_slice(iv);
        Ripemd {
            h,
            n,
            count: [0; 2],
            b: BlockBuf::new(),
        }
    }
}

impl Engine for Ripemd {
    fn update(&mut self, data: &[u8]) {
        let used = add_bits(&mut self.count, data.len(), 64);
        let (h, n) = (&mut self.h, self.n);
        self.b.absorb(used, data, true, |blk| compress(h, n, blk));
    }

    fn finish(&mut self, out: &mut [u8]) {
        let bits = ((self.count[1] as u64) << 32) | self.count[0] as u64;
        let used = (self.count[0] >> 3) as usize % 64;
        let pad = if used < 56 { 56 - used } else { 120 - used };
        let mut p = [0u8; 72];
        p[0] = 0x80;
        p[pad..pad + 8].copy_from_slice(&bits.to_le_bytes());
        self.update(&p[..pad + 8]);
        for i in 0..self.n {
            out[i * 4..i * 4 + 4].copy_from_slice(&self.h[i].to_le_bytes());
        }
    }

    clone_box!();

    fn export(&self) -> Vec<Val> {
        let mut v: Vec<Val> = self.h[..self.n].iter().map(|&w| Val::U32(w)).collect();
        v.push(Val::U32(self.count[0]));
        v.push(Val::U32(self.count[1]));
        v.push(Val::Bytes(self.b.buf.to_vec()));
        v
    }

    fn import(&mut self, v: &[Val]) -> bool {
        let n = self.n;
        for i in 0..n {
            self.h[i] = v[i].u32();
        }
        self.count = [v[n].u32(), v[n + 1].u32()];
        self.b.buf.copy_from_slice(v[n + 2].bytes());
        true
    }
}
