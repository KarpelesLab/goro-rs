//! RIPEMD-128/160/256/320 (ext/hash/hash_ripemd.c).

use super::md::md32_le_final;
use super::{le_words, md32_update};
use crate::ops::{Ctx, Img, InitArgs, rd_bytes, rd_u32s};

fn f0(x: u32, y: u32, z: u32) -> u32 {
    x ^ y ^ z
}
fn f1(x: u32, y: u32, z: u32) -> u32 {
    (x & y) | (!x & z)
}
fn f2(x: u32, y: u32, z: u32) -> u32 {
    (x | !y) ^ z
}
fn f3(x: u32, y: u32, z: u32) -> u32 {
    (x & z) | (y & !z)
}
fn f4(x: u32, y: u32, z: u32) -> u32 {
    x ^ (y | !z)
}

const K_VALUES: [u32; 5] = [0x00000000, 0x5A827999, 0x6ED9EBA1, 0x8F1BBCDC, 0xA953FD4E];
const KK_VALUES: [u32; 4] = [0x50A28BE6, 0x5C4DD124, 0x6D703EF3, 0x00000000];
const KK160_VALUES: [u32; 5] = [0x50A28BE6, 0x5C4DD124, 0x6D703EF3, 0x7A6D76E9, 0x00000000];

fn k(n: usize) -> u32 {
    K_VALUES[n >> 4]
}
fn kk(n: usize) -> u32 {
    KK_VALUES[n >> 4]
}
fn kk160(n: usize) -> u32 {
    KK160_VALUES[n >> 4]
}

static R: [usize; 80] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 7, 4, 13, 1, 10, 6, 15, 3, 12, 0, 9, 5,
    2, 14, 11, 8, 3, 10, 14, 4, 9, 15, 8, 1, 2, 7, 0, 6, 13, 11, 5, 12, 1, 9, 11, 10, 0, 8, 12, 4,
    13, 3, 7, 15, 14, 5, 6, 2, 4, 0, 5, 9, 7, 12, 2, 10, 14, 1, 3, 8, 11, 6, 15, 13,
];
static RR: [usize; 80] = [
    5, 14, 7, 0, 9, 2, 11, 4, 13, 6, 15, 8, 1, 10, 3, 12, 6, 11, 3, 7, 0, 13, 5, 10, 14, 15, 8, 12,
    4, 9, 1, 2, 15, 5, 1, 3, 7, 14, 6, 9, 11, 8, 12, 2, 10, 0, 4, 13, 8, 6, 4, 1, 3, 11, 15, 0, 5,
    12, 2, 13, 9, 7, 10, 14, 12, 15, 10, 4, 1, 5, 8, 7, 6, 2, 13, 14, 0, 3, 9, 11,
];
static S: [u32; 80] = [
    11, 14, 15, 12, 5, 8, 7, 9, 11, 13, 14, 15, 6, 7, 9, 8, 7, 6, 8, 13, 11, 9, 7, 15, 7, 12, 15,
    9, 11, 7, 13, 12, 11, 13, 6, 7, 14, 9, 13, 15, 14, 8, 13, 6, 5, 12, 7, 5, 11, 12, 14, 15, 14,
    15, 9, 8, 9, 14, 5, 6, 8, 6, 5, 12, 9, 15, 5, 11, 6, 8, 13, 12, 5, 12, 13, 14, 11, 8, 5, 6,
];
static SS: [u32; 80] = [
    8, 9, 9, 11, 13, 15, 15, 5, 7, 7, 8, 11, 14, 14, 12, 6, 9, 13, 15, 7, 12, 8, 9, 11, 7, 7, 12,
    7, 6, 15, 13, 11, 9, 7, 15, 11, 8, 6, 6, 14, 12, 13, 5, 14, 13, 13, 7, 5, 15, 5, 8, 11, 14, 14,
    6, 14, 6, 9, 12, 9, 12, 5, 15, 8, 8, 5, 12, 9, 12, 5, 14, 6, 8, 13, 6, 5, 15, 13, 11, 11,
];

type F = fn(u32, u32, u32) -> u32;

/// Rounds of the 4-word lines (RIPEMD-128/256).
#[allow(clippy::too_many_arguments)]
fn line4(
    j: usize,
    f: F,
    ff: F,
    x: &[u32; 16],
    a: &mut u32,
    b: &mut u32,
    c: &mut u32,
    d: &mut u32,
    aa: &mut u32,
    bb: &mut u32,
    cc: &mut u32,
    dd: &mut u32,
) {
    let tmp = a
        .wrapping_add(f(*b, *c, *d))
        .wrapping_add(x[R[j]])
        .wrapping_add(k(j))
        .rotate_left(S[j]);
    *a = *d;
    *d = *c;
    *c = *b;
    *b = tmp;
    let tmp = aa
        .wrapping_add(ff(*bb, *cc, *dd))
        .wrapping_add(x[RR[j]])
        .wrapping_add(kk(j))
        .rotate_left(SS[j]);
    *aa = *dd;
    *dd = *cc;
    *cc = *bb;
    *bb = tmp;
}

fn r128_transform(state: &mut [u32; 4], block: &[u8]) {
    let x: [u32; 16] = le_words(block);
    let (mut a, mut b, mut c, mut d) = (state[0], state[1], state[2], state[3]);
    let (mut aa, mut bb, mut cc, mut dd) = (state[0], state[1], state[2], state[3]);
    let fs: [(F, F); 4] = [(f0, f3), (f1, f2), (f2, f1), (f3, f0)];
    for (round, (f, ff)) in fs.iter().enumerate() {
        for j in 16 * round..16 * round + 16 {
            line4(
                j, *f, *ff, &x, &mut a, &mut b, &mut c, &mut d, &mut aa, &mut bb, &mut cc, &mut dd,
            );
        }
    }
    let tmp = state[1].wrapping_add(c).wrapping_add(dd);
    state[1] = state[2].wrapping_add(d).wrapping_add(aa);
    state[2] = state[3].wrapping_add(a).wrapping_add(bb);
    state[3] = state[0].wrapping_add(b).wrapping_add(cc);
    state[0] = tmp;
}

fn r256_transform(state: &mut [u32; 8], block: &[u8]) {
    let x: [u32; 16] = le_words(block);
    let (mut a, mut b, mut c, mut d) = (state[0], state[1], state[2], state[3]);
    let (mut aa, mut bb, mut cc, mut dd) = (state[4], state[5], state[6], state[7]);
    let fs: [(F, F); 4] = [(f0, f3), (f1, f2), (f2, f1), (f3, f0)];
    for (round, (f, ff)) in fs.iter().enumerate() {
        for j in 16 * round..16 * round + 16 {
            line4(
                j, *f, *ff, &x, &mut a, &mut b, &mut c, &mut d, &mut aa, &mut bb, &mut cc, &mut dd,
            );
        }
        match round {
            0 => std::mem::swap(&mut a, &mut aa),
            1 => std::mem::swap(&mut b, &mut bb),
            2 => std::mem::swap(&mut c, &mut cc),
            _ => std::mem::swap(&mut d, &mut dd),
        }
    }
    for (s, v) in state.iter_mut().zip([a, b, c, d, aa, bb, cc, dd]) {
        *s = s.wrapping_add(v);
    }
}

/// Rounds of the 5-word lines (RIPEMD-160/320).
fn line5(j: usize, f: F, ff: F, x: &[u32; 16], l: &mut [u32; 5], r: &mut [u32; 5]) {
    let [a, b, c, d, e] = *l;
    let tmp = a
        .wrapping_add(f(b, c, d))
        .wrapping_add(x[R[j]])
        .wrapping_add(k(j))
        .rotate_left(S[j])
        .wrapping_add(e);
    *l = [e, tmp, b, c.rotate_left(10), d];
    let [aa, bb, cc, dd, ee] = *r;
    let tmp = aa
        .wrapping_add(ff(bb, cc, dd))
        .wrapping_add(x[RR[j]])
        .wrapping_add(kk160(j))
        .rotate_left(SS[j])
        .wrapping_add(ee);
    *r = [ee, tmp, bb, cc.rotate_left(10), dd];
}

static FS5: [(F, F); 5] = [(f0, f4), (f1, f3), (f2, f2), (f3, f1), (f4, f0)];

fn r160_transform(state: &mut [u32; 5], block: &[u8]) {
    let x: [u32; 16] = le_words(block);
    let mut l = *state;
    let mut r = *state;
    for (round, (f, ff)) in FS5.iter().enumerate() {
        for j in 16 * round..16 * round + 16 {
            line5(j, *f, *ff, &x, &mut l, &mut r);
        }
    }
    let [a, b, c, d, e] = l;
    let [aa, bb, cc, dd, ee] = r;
    let tmp = state[1].wrapping_add(c).wrapping_add(dd);
    state[1] = state[2].wrapping_add(d).wrapping_add(ee);
    state[2] = state[3].wrapping_add(e).wrapping_add(aa);
    state[3] = state[4].wrapping_add(a).wrapping_add(bb);
    state[4] = state[0].wrapping_add(b).wrapping_add(cc);
    state[0] = tmp;
}

fn r320_transform(state: &mut [u32; 10], block: &[u8]) {
    let x: [u32; 16] = le_words(block);
    let mut l: [u32; 5] = state[..5].try_into().unwrap();
    let mut r: [u32; 5] = state[5..].try_into().unwrap();
    for (round, (f, ff)) in FS5.iter().enumerate() {
        for j in 16 * round..16 * round + 16 {
            line5(j, *f, *ff, &x, &mut l, &mut r);
        }
        // swap b, d, a, c, e with their right-line counterparts
        let idx = [1, 3, 0, 2, 4][round];
        std::mem::swap(&mut l[idx], &mut r[idx]);
    }
    for (k, s) in state.iter_mut().enumerate() {
        let v = if k < 5 { l[k] } else { r[k - 5] };
        *s = s.wrapping_add(v);
    }
}

#[derive(Clone)]
pub struct Ripemd<const N: usize> {
    state: [u32; N],
    count: [u32; 2],
    buffer: [u8; 64],
    transform: fn(&mut [u32; N], &[u8]),
}

fn new_ripemd<const N: usize>(
    state: [u32; N],
    transform: fn(&mut [u32; N], &[u8]),
) -> Box<dyn Ctx> {
    Box::new(Ripemd {
        state,
        count: [0, 0],
        buffer: [0; 64],
        transform,
    })
}

pub fn r128_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    new_ripemd(
        [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476],
        r128_transform,
    )
}

pub fn r160_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    new_ripemd(
        [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0],
        r160_transform,
    )
}

pub fn r256_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    new_ripemd(
        [
            0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0x76543210, 0xFEDCBA98, 0x89ABCDEF,
            0x01234567,
        ],
        r256_transform,
    )
}

pub fn r320_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    new_ripemd(
        [
            0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0, 0x76543210, 0xFEDCBA98,
            0x89ABCDEF, 0x01234567, 0x3C2D1E0F,
        ],
        r320_transform,
    )
}

impl<const N: usize> Ctx for Ripemd<N> {
    fn update(&mut self, data: &[u8]) {
        let st = &mut self.state;
        let tf = self.transform;
        md32_update(&mut self.count, &mut self.buffer, data, |b| tf(st, b));
    }

    fn finish(&mut self, out: &mut [u8]) {
        md32_le_final(
            &mut self.state,
            &mut self.count,
            &mut self.buffer,
            &mut out[..4 * N],
            self.transform,
        );
    }

    fn image(&self) -> Vec<u8> {
        let mut v = vec![0u8; 4 * N + 72];
        let mut w = Img(&mut v);
        w.u32s(0, &self.state);
        w.u32s(4 * N, &self.count);
        w.bytes(4 * N + 8, &self.buffer);
        v
    }

    fn set_image(&mut self, img: &[u8]) {
        self.state = rd_u32s(img, 0);
        self.count = rd_u32s(img, 4 * N);
        self.buffer = rd_bytes(img, 4 * N + 8);
    }
}
