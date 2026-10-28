//! HAVAL (Zheng, Pieprzyk, Seberry, AUSCRYPT'92) with 3, 4 or 5 passes
//! and 128 to 256 bits of output, following the designers' reference
//! implementation (haval.c, V.1, June 1993).
//!
//! Serialized layout observed on PHP 8.5: eight state words, the 64-bit
//! bit count (low word first) and the 128-byte buffer.

use crate::engine::{BlockBuf, Engine, Field, Kind, Val, clone_box, f};
use crate::md::add_bits;

pub(crate) const HAVAL_LAYOUT: &[Field] = &[
    f(Kind::U32, 8, 0),
    f(Kind::U32, 2, 32),
    f(Kind::Bytes(128), 1, 40),
];

const IV: [u32; 8] = [
    0x243f6a88, 0x85a308d3, 0x13198a2e, 0x03707344, 0xa4093822, 0x299f31d0, 0x082efa98, 0xec4e6c89,
];

#[rustfmt::skip]
const ORDER: [[usize; 32]; 5] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
     16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31],
    [5, 14, 26, 18, 11, 28, 7, 16, 0, 23, 20, 22, 1, 10, 4, 8,
     30, 3, 21, 9, 17, 24, 29, 6, 19, 12, 15, 13, 2, 25, 31, 27],
    [19, 9, 4, 20, 28, 17, 8, 22, 29, 14, 25, 12, 24, 30, 16, 26,
     31, 15, 7, 3, 1, 0, 18, 27, 13, 6, 21, 10, 23, 11, 5, 2],
    [24, 4, 0, 14, 2, 7, 28, 23, 26, 6, 30, 20, 18, 25, 19, 3,
     22, 11, 31, 21, 8, 27, 12, 9, 1, 29, 5, 15, 17, 10, 16, 13],
    [27, 3, 21, 26, 17, 11, 20, 29, 19, 0, 12, 7, 13, 8, 31, 10,
     5, 9, 14, 30, 18, 6, 28, 24, 2, 23, 16, 22, 4, 1, 25, 15],
];

#[rustfmt::skip]
const KC: [[u32; 32]; 5] = [
    [0; 32],
    [0x452821e6, 0x38d01377, 0xbe5466cf, 0x34e90c6c, 0xc0ac29b7, 0xc97c50dd, 0x3f84d5b5, 0xb5470917,
     0x9216d5d9, 0x8979fb1b, 0xd1310ba6, 0x98dfb5ac, 0x2ffd72db, 0xd01adfb7, 0xb8e1afed, 0x6a267e96,
     0xba7c9045, 0xf12c7f99, 0x24a19947, 0xb3916cf7, 0x0801f2e2, 0x858efc16, 0x636920d8, 0x71574e69,
     0xa458fea3, 0xf4933d7e, 0x0d95748f, 0x728eb658, 0x718bcd58, 0x82154aee, 0x7b54a41d, 0xc25a59b5],
    [0x9c30d539, 0x2af26013, 0xc5d1b023, 0x286085f0, 0xca417918, 0xb8db38ef, 0x8e79dcb0, 0x603a180e,
     0x6c9e0e8b, 0xb01e8a3e, 0xd71577c1, 0xbd314b27, 0x78af2fda, 0x55605c60, 0xe65525f3, 0xaa55ab94,
     0x57489862, 0x63e81440, 0x55ca396a, 0x2aab10b6, 0xb4cc5c34, 0x1141e8ce, 0xa15486af, 0x7c72e993,
     0xb3ee1411, 0x636fbc2a, 0x2ba9c55d, 0x741831f6, 0xce5c3e16, 0x9b87931e, 0xafd6ba33, 0x6c24cf5c],
    [0x7a325381, 0x28958677, 0x3b8f4898, 0x6b4bb9af, 0xc4bfe81b, 0x66282193, 0x61d809cc, 0xfb21a991,
     0x487cac60, 0x5dec8032, 0xef845d5d, 0xe98575b1, 0xdc262302, 0xeb651b88, 0x23893e81, 0xd396acc5,
     0x0f6d6ff3, 0x83f44239, 0x2e0b4482, 0xa4842004, 0x69c8f04a, 0x9e1f9b5e, 0x21c66842, 0xf6e96c9a,
     0x670c9c61, 0xabd388f0, 0x6a51a0d2, 0xd8542f68, 0x960fa728, 0xab5133a3, 0x6eef0b6c, 0x137a3be4],
    [0xba3bf050, 0x7efb2a98, 0xa1f1651d, 0x39af0176, 0x66ca593e, 0x82430e88, 0x8cee8619, 0x456f9fb4,
     0x7d84a5c3, 0x3b8b5ebe, 0xe06f75d8, 0x85c12073, 0x401a449f, 0x56c16aa6, 0x4ed3aa62, 0x363f7706,
     0x1bfedf72, 0x429b023d, 0x37d0d724, 0xd00a1248, 0xdb0fead3, 0x49f1c09b, 0x075372c9, 0x80991b7b,
     0x25d479d8, 0xf6e8def7, 0xe3fe501a, 0xb6794c3b, 0x976ce0bd, 0x04c006ba, 0xc1a94fb6, 0x409f60c4],
];

/// phi_{passes,k}: which of x6..x0 feed f_k's arguments (x6..x0 order).
#[rustfmt::skip]
const PHI: [[[usize; 7]; 5]; 3] = [
    [[1, 0, 3, 5, 6, 2, 4], [4, 2, 1, 0, 5, 3, 6], [6, 1, 2, 3, 4, 5, 0], [0; 7], [0; 7]],
    [[2, 6, 1, 4, 5, 3, 0], [3, 5, 2, 0, 1, 6, 4], [1, 4, 3, 6, 0, 2, 5], [6, 4, 0, 5, 2, 1, 3], [0; 7]],
    [[3, 4, 1, 0, 5, 2, 6], [6, 2, 1, 0, 3, 4, 5], [2, 6, 0, 4, 3, 1, 5], [1, 5, 3, 2, 0, 4, 6],
     [2, 5, 0, 6, 4, 3, 1]],
];

#[inline(always)]
fn fk(k: usize, x6: u32, x5: u32, x4: u32, x3: u32, x2: u32, x1: u32, x0: u32) -> u32 {
    match k {
        0 => (x1 & (x0 ^ x4)) ^ (x2 & x5) ^ (x3 & x6) ^ x0,
        1 => (x2 & ((x1 & !x3) ^ (x4 & x5) ^ x6 ^ x0)) ^ (x4 & (x1 ^ x5)) ^ (x3 & x5) ^ x0,
        2 => (x3 & ((x1 & x2) ^ x6 ^ x0)) ^ (x1 & x4) ^ (x2 & x5) ^ x0,
        3 => {
            (x4 & ((x5 & !x2) ^ (x3 & !x6) ^ x1 ^ x6 ^ x0))
                ^ (x3 & ((x1 & x2) ^ x5 ^ x6))
                ^ (x2 & x6)
                ^ x0
        }
        _ => (x0 & ((x1 & x2 & x3) ^ !x5)) ^ (x1 & x4) ^ (x2 & x5) ^ (x3 & x6),
    }
}

/// One step of pass `K` (of `P` passes) whose position in its group of
/// eight is `S`: the registers rotate by one each step, so that
/// x_i = t[(i - S) mod 8] and the step updates x_7.
#[inline(always)]
fn step<const P: usize, const K: usize, const S: usize>(t: &mut [u32; 8], w: u32, kc: u32) {
    let x = |i: usize| t[(i + 8 - S) % 8];
    let a = [x(6), x(5), x(4), x(3), x(2), x(1), x(0)];
    // PHI lists, for each of f's (x6..x0) arguments, which x feeds it.
    let perm = &PHI[P - 3][K];
    let sel = |n: usize| a[6 - perm[n]];
    let v = fk(K, sel(0), sel(1), sel(2), sel(3), sel(4), sel(5), sel(6));
    let target = (15 - S) % 8;
    t[target] = v
        .rotate_right(7)
        .wrapping_add(t[target].rotate_right(11))
        .wrapping_add(w)
        .wrapping_add(kc);
}

#[inline(always)]
fn pass<const P: usize, const K: usize>(t: &mut [u32; 8], w: &[u32; 32]) {
    for g in 0..4 {
        let j = 8 * g;
        step::<P, K, 0>(t, w[ORDER[K][j]], KC[K][j]);
        step::<P, K, 1>(t, w[ORDER[K][j + 1]], KC[K][j + 1]);
        step::<P, K, 2>(t, w[ORDER[K][j + 2]], KC[K][j + 2]);
        step::<P, K, 3>(t, w[ORDER[K][j + 3]], KC[K][j + 3]);
        step::<P, K, 4>(t, w[ORDER[K][j + 4]], KC[K][j + 4]);
        step::<P, K, 5>(t, w[ORDER[K][j + 5]], KC[K][j + 5]);
        step::<P, K, 6>(t, w[ORDER[K][j + 6]], KC[K][j + 6]);
        step::<P, K, 7>(t, w[ORDER[K][j + 7]], KC[K][j + 7]);
    }
}

fn compress_p<const P: usize>(h: &mut [u32; 8], block: &[u8; 128]) {
    let (words, _) = block.as_chunks::<4>();
    let w: [u32; 32] = std::array::from_fn(|i| u32::from_le_bytes(words[i]));
    let mut t = *h;
    pass::<P, 0>(&mut t, &w);
    pass::<P, 1>(&mut t, &w);
    pass::<P, 2>(&mut t, &w);
    if P >= 4 {
        pass::<P, 3>(&mut t, &w);
    }
    if P == 5 {
        pass::<P, 4>(&mut t, &w);
    }
    for i in 0..8 {
        h[i] = h[i].wrapping_add(t[i]);
    }
}

fn compress(h: &mut [u32; 8], block: &[u8; 128], passes: usize) {
    match passes {
        3 => compress_p::<3>(h, block),
        4 => compress_p::<4>(h, block),
        _ => compress_p::<5>(h, block),
    }
}

fn tailor(h: &mut [u32; 8], bits: usize) {
    let f = *h;
    match bits {
        128 => {
            let t = (f[7] & 0x000000ff)
                | (f[6] & 0xff000000)
                | (f[5] & 0x00ff0000)
                | (f[4] & 0x0000ff00);
            h[0] = h[0].wrapping_add(t.rotate_right(8));
            let t = (f[7] & 0x0000ff00)
                | (f[6] & 0x000000ff)
                | (f[5] & 0xff000000)
                | (f[4] & 0x00ff0000);
            h[1] = h[1].wrapping_add(t.rotate_right(16));
            let t = (f[7] & 0x00ff0000)
                | (f[6] & 0x0000ff00)
                | (f[5] & 0x000000ff)
                | (f[4] & 0xff000000);
            h[2] = h[2].wrapping_add(t.rotate_right(24));
            let t = (f[7] & 0xff000000)
                | (f[6] & 0x00ff0000)
                | (f[5] & 0x0000ff00)
                | (f[4] & 0x000000ff);
            h[3] = h[3].wrapping_add(t);
        }
        160 => {
            let t = (f[7] & 0x3f) | (f[6] & (0x7f << 25)) | (f[5] & (0x3f << 19));
            h[0] = h[0].wrapping_add(t.rotate_right(19));
            let t = (f[7] & (0x3f << 6)) | (f[6] & 0x3f) | (f[5] & (0x7f << 25));
            h[1] = h[1].wrapping_add(t.rotate_right(25));
            let t = (f[7] & (0x7f << 12)) | (f[6] & (0x3f << 6)) | (f[5] & 0x3f);
            h[2] = h[2].wrapping_add(t);
            let t = (f[7] & (0x3f << 19)) | (f[6] & (0x7f << 12)) | (f[5] & (0x3f << 6));
            h[3] = h[3].wrapping_add(t >> 6);
            let t = (f[7] & (0x7f << 25)) | (f[6] & (0x3f << 19)) | (f[5] & (0x7f << 12));
            h[4] = h[4].wrapping_add(t >> 12);
        }
        192 => {
            let t = (f[7] & 0x1f) | (f[6] & (0x3f << 26));
            h[0] = h[0].wrapping_add(t.rotate_right(26));
            let t = (f[7] & (0x1f << 5)) | (f[6] & 0x1f);
            h[1] = h[1].wrapping_add(t);
            let t = (f[7] & (0x3f << 10)) | (f[6] & (0x1f << 5));
            h[2] = h[2].wrapping_add(t >> 5);
            let t = (f[7] & (0x1f << 16)) | (f[6] & (0x3f << 10));
            h[3] = h[3].wrapping_add(t >> 10);
            let t = (f[7] & (0x1f << 21)) | (f[6] & (0x1f << 16));
            h[4] = h[4].wrapping_add(t >> 16);
            let t = (f[7] & (0x3f << 26)) | (f[6] & (0x1f << 21));
            h[5] = h[5].wrapping_add(t >> 21);
        }
        224 => {
            h[0] = h[0].wrapping_add((f[7] >> 27) & 0x1f);
            h[1] = h[1].wrapping_add((f[7] >> 22) & 0x1f);
            h[2] = h[2].wrapping_add((f[7] >> 18) & 0x0f);
            h[3] = h[3].wrapping_add((f[7] >> 13) & 0x1f);
            h[4] = h[4].wrapping_add((f[7] >> 9) & 0x0f);
            h[5] = h[5].wrapping_add((f[7] >> 4) & 0x1f);
            h[6] = h[6].wrapping_add(f[7] & 0x0f);
        }
        _ => {}
    }
}

#[derive(Clone)]
pub(crate) struct Haval {
    h: [u32; 8],
    count: [u32; 2],
    b: BlockBuf<128>,
    passes: usize,
    bits: usize,
}

impl Haval {
    pub(crate) fn new(bits: usize, passes: usize) -> Self {
        Haval {
            h: IV,
            count: [0; 2],
            b: BlockBuf::new(),
            passes,
            bits,
        }
    }
}

impl Engine for Haval {
    fn update(&mut self, data: &[u8]) {
        let used = add_bits(&mut self.count, data.len(), 128);
        let (h, p) = (&mut self.h, self.passes);
        self.b.absorb(used, data, true, |blks| {
            blks.iter().for_each(|b| compress(h, b, p))
        });
    }

    fn finish(&mut self, out: &mut [u8]) {
        let mut tail = [0u8; 10];
        tail[0] = (((self.bits & 3) << 6) | ((self.passes & 7) << 3) | 1) as u8;
        tail[1] = (self.bits >> 2) as u8;
        tail[2..6].copy_from_slice(&self.count[0].to_le_bytes());
        tail[6..10].copy_from_slice(&self.count[1].to_le_bytes());
        let used = (self.count[0] >> 3) as usize & 0x7f;
        let pad = if used < 118 { 118 - used } else { 246 - used };
        let mut p = [0u8; 256];
        p[0] = 0x01;
        self.update(&p[..pad]);
        self.update(&tail);
        tailor(&mut self.h, self.bits);
        for i in 0..self.bits / 32 {
            out[i * 4..i * 4 + 4].copy_from_slice(&self.h[i].to_le_bytes());
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
        for i in 0..8 {
            self.h[i] = v[i].u32();
        }
        self.count = [v[8].u32(), v[9].u32()];
        self.b.buf.copy_from_slice(v[10].bytes());
        true
    }
}
