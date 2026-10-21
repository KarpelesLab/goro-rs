//! SHA-1 (ext/standard/sha1.c) and SHA-2 (ext/hash/hash_sha.c).

use super::{PADDING, be_words, encode_be, md32_update, md64_update};
use crate::ops::{Ctx, Img, InitArgs, rd_bytes, rd_u32s, rd_u64s};

// ---------------------------------------------------------------- SHA-1

#[derive(Clone)]
pub struct Sha1 {
    state: [u32; 5],
    count: [u32; 2],
    buffer: [u8; 64],
}

impl Sha1 {
    pub fn new() -> Sha1 {
        Sha1 {
            state: [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0],
            count: [0, 0],
            buffer: [0; 64],
        }
    }
}

impl Default for Sha1 {
    fn default() -> Self {
        Self::new()
    }
}

pub fn sha1_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    Box::new(Sha1::new())
}

fn sha1_transform(state: &mut [u32; 5], block: &[u8]) {
    let mut w = [0u32; 80];
    w[..16].copy_from_slice(&be_words::<16>(block));
    for i in 16..80 {
        w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
    }
    let [mut a, mut b, mut c, mut d, mut e] = *state;
    for (i, wi) in w.iter().enumerate() {
        let (f, k) = match i {
            0..=19 => ((b & c) | (!b & d), 0x5A827999u32),
            20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
            40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
            _ => (b ^ c ^ d, 0xCA62C1D6),
        };
        let t = a
            .rotate_left(5)
            .wrapping_add(f)
            .wrapping_add(e)
            .wrapping_add(k)
            .wrapping_add(*wi);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = t;
    }
    for (s, v) in state.iter_mut().zip([a, b, c, d, e]) {
        *s = s.wrapping_add(v);
    }
}

/// Final of the big-endian 32-bit MD family (SHA-1, SHA-224/256).
fn md32_be_final<const S: usize>(
    state: &mut [u32; S],
    count: &mut [u32; 2],
    buffer: &mut [u8; 64],
    out: &mut [u8],
    tf: fn(&mut [u32; S], &[u8]),
) {
    let mut bits = [0u8; 8];
    bits[..4].copy_from_slice(&count[1].to_be_bytes());
    bits[4..].copy_from_slice(&count[0].to_be_bytes());
    let index = ((count[0] >> 3) & 0x3f) as usize;
    let pad_len = if index < 56 { 56 - index } else { 120 - index };
    md32_update(count, buffer, &PADDING[..pad_len], |b| tf(state, b));
    md32_update(count, buffer, &bits, |b| tf(state, b));
    encode_be(out, &state[..]);
}

impl Ctx for Sha1 {
    fn update(&mut self, data: &[u8]) {
        let st = &mut self.state;
        md32_update(&mut self.count, &mut self.buffer, data, |b| {
            sha1_transform(st, b)
        });
    }

    fn finish(&mut self, out: &mut [u8]) {
        md32_be_final(
            &mut self.state,
            &mut self.count,
            &mut self.buffer,
            &mut out[..20],
            sha1_transform,
        );
    }

    fn image(&self) -> Vec<u8> {
        let mut v = vec![0u8; 92];
        let mut w = Img(&mut v);
        w.u32s(0, &self.state);
        w.u32s(20, &self.count);
        w.bytes(28, &self.buffer);
        v
    }

    fn set_image(&mut self, img: &[u8]) {
        self.state = rd_u32s(img, 0);
        self.count = rd_u32s(img, 20);
        self.buffer = rd_bytes(img, 28);
    }
}

/// sha1() helper.
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut c = Sha1::new();
    c.update(data);
    let mut out = [0u8; 20];
    c.finish(&mut out);
    out
}

// ---------------------------------------------------------------- SHA-224/256

static K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn sha256_transform(state: &mut [u32; 8], block: &[u8]) {
    let mut w = [0u32; 64];
    w[..16].copy_from_slice(&be_words::<16>(block));
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let t1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K256[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (s, v) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *s = s.wrapping_add(v);
    }
}

#[derive(Clone)]
pub struct Sha256 {
    state: [u32; 8],
    count: [u32; 2],
    buffer: [u8; 64],
    digest: usize,
}

pub fn sha224_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    Box::new(Sha256 {
        state: [
            0xc1059ed8, 0x367cd507, 0x3070dd17, 0xf70e5939, 0xffc00b31, 0x68581511, 0x64f98fa7,
            0xbefa4fa4,
        ],
        count: [0, 0],
        buffer: [0; 64],
        digest: 28,
    })
}

pub fn sha256_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    Box::new(Sha256 {
        state: [
            0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
            0x5be0cd19,
        ],
        count: [0, 0],
        buffer: [0; 64],
        digest: 32,
    })
}

impl Ctx for Sha256 {
    fn update(&mut self, data: &[u8]) {
        let st = &mut self.state;
        md32_update(&mut self.count, &mut self.buffer, data, |b| {
            sha256_transform(st, b)
        });
    }

    fn finish(&mut self, out: &mut [u8]) {
        md32_be_final(
            &mut self.state,
            &mut self.count,
            &mut self.buffer,
            &mut out[..self.digest],
            sha256_transform,
        );
    }

    fn image(&self) -> Vec<u8> {
        let mut v = vec![0u8; 104];
        let mut w = Img(&mut v);
        w.u32s(0, &self.state);
        w.u32s(32, &self.count);
        w.bytes(40, &self.buffer);
        v
    }

    fn set_image(&mut self, img: &[u8]) {
        self.state = rd_u32s(img, 0);
        self.count = rd_u32s(img, 32);
        self.buffer = rd_bytes(img, 40);
    }
}

// ---------------------------------------------------------------- SHA-384/512

static K512: [u64; 80] = [
    0x428a2f98d728ae22,
    0x7137449123ef65cd,
    0xb5c0fbcfec4d3b2f,
    0xe9b5dba58189dbbc,
    0x3956c25bf348b538,
    0x59f111f1b605d019,
    0x923f82a4af194f9b,
    0xab1c5ed5da6d8118,
    0xd807aa98a3030242,
    0x12835b0145706fbe,
    0x243185be4ee4b28c,
    0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f,
    0x80deb1fe3b1696b1,
    0x9bdc06a725c71235,
    0xc19bf174cf692694,
    0xe49b69c19ef14ad2,
    0xefbe4786384f25e3,
    0x0fc19dc68b8cd5b5,
    0x240ca1cc77ac9c65,
    0x2de92c6f592b0275,
    0x4a7484aa6ea6e483,
    0x5cb0a9dcbd41fbd4,
    0x76f988da831153b5,
    0x983e5152ee66dfab,
    0xa831c66d2db43210,
    0xb00327c898fb213f,
    0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2,
    0xd5a79147930aa725,
    0x06ca6351e003826f,
    0x142929670a0e6e70,
    0x27b70a8546d22ffc,
    0x2e1b21385c26c926,
    0x4d2c6dfc5ac42aed,
    0x53380d139d95b3df,
    0x650a73548baf63de,
    0x766a0abb3c77b2a8,
    0x81c2c92e47edaee6,
    0x92722c851482353b,
    0xa2bfe8a14cf10364,
    0xa81a664bbc423001,
    0xc24b8b70d0f89791,
    0xc76c51a30654be30,
    0xd192e819d6ef5218,
    0xd69906245565a910,
    0xf40e35855771202a,
    0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8,
    0x1e376c085141ab53,
    0x2748774cdf8eeb99,
    0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63,
    0x4ed8aa4ae3418acb,
    0x5b9cca4f7763e373,
    0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc,
    0x78a5636f43172f60,
    0x84c87814a1f0ab72,
    0x8cc702081a6439ec,
    0x90befffa23631e28,
    0xa4506cebde82bde9,
    0xbef9a3f7b2c67915,
    0xc67178f2e372532b,
    0xca273eceea26619c,
    0xd186b8c721c0c207,
    0xeada7dd6cde0eb1e,
    0xf57d4f7fee6ed178,
    0x06f067aa72176fba,
    0x0a637dc5a2c898a6,
    0x113f9804bef90dae,
    0x1b710b35131c471b,
    0x28db77f523047d84,
    0x32caab7b40c72493,
    0x3c9ebe0a15c9bebc,
    0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6,
    0x597f299cfc657e2a,
    0x5fcb6fab3ad6faec,
    0x6c44198c4a475817,
];

fn sha512_transform(state: &mut [u64; 8], block: &[u8]) {
    let mut w = [0u64; 80];
    for (i, x) in w.iter_mut().take(16).enumerate() {
        *x = u64::from_be_bytes(block[8 * i..8 * i + 8].try_into().unwrap());
    }
    for i in 16..80 {
        let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
        let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for i in 0..80 {
        let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
        let ch = (e & f) ^ (!e & g);
        let t1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K512[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (s, v) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *s = s.wrapping_add(v);
    }
}

#[derive(Clone)]
pub struct Sha512 {
    state: [u64; 8],
    count: [u64; 2],
    buffer: [u8; 128],
    digest: usize,
}

fn sha512_with(state: [u64; 8], digest: usize) -> Box<dyn Ctx> {
    Box::new(Sha512 {
        state,
        count: [0, 0],
        buffer: [0; 128],
        digest,
    })
}

pub fn sha384_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    sha512_with(
        [
            0xcbbb9d5dc1059ed8,
            0x629a292a367cd507,
            0x9159015a3070dd17,
            0x152fecd8f70e5939,
            0x67332667ffc00b31,
            0x8eb44a8768581511,
            0xdb0c2e0d64f98fa7,
            0x47b5481dbefa4fa4,
        ],
        48,
    )
}

pub fn sha512_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    sha512_with(
        [
            0x6a09e667f3bcc908,
            0xbb67ae8584caa73b,
            0x3c6ef372fe94f82b,
            0xa54ff53a5f1d36f1,
            0x510e527fade682d1,
            0x9b05688c2b3e6c1f,
            0x1f83d9abfb41bd6b,
            0x5be0cd19137e2179,
        ],
        64,
    )
}

pub fn sha512_256_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    sha512_with(
        [
            0x22312194FC2BF72C,
            0x9F555FA3C84C64C2,
            0x2393B86B6F53B151,
            0x963877195940EABD,
            0x96283EE2A88EFFE3,
            0xBE5E1E2553863992,
            0x2B0199FC2C85B8AA,
            0x0EB72DDC81C52CA2,
        ],
        32,
    )
}

pub fn sha512_224_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    sha512_with(
        [
            0x8C3D37C819544DA2,
            0x73E1996689DCD4D6,
            0x1DFAB7AE32FF9C82,
            0x679DD514582F9FCF,
            0x0F6D2B697BD44DA8,
            0x77E36F7304C48942,
            0x3F9D85A86A1D36C8,
            0x1112E6AD91D692A1,
        ],
        28,
    )
}

impl Ctx for Sha512 {
    fn update(&mut self, data: &[u8]) {
        let st = &mut self.state;
        md64_update(&mut self.count, &mut self.buffer, data, |b| {
            sha512_transform(st, b)
        });
    }

    fn finish(&mut self, out: &mut [u8]) {
        let mut bits = [0u8; 16];
        bits[..8].copy_from_slice(&self.count[1].to_be_bytes());
        bits[8..].copy_from_slice(&self.count[0].to_be_bytes());
        let index = ((self.count[0] >> 3) & 0x7f) as usize;
        let pad_len = if index < 112 {
            112 - index
        } else {
            240 - index
        };
        self.update(&PADDING[..pad_len]);
        self.update(&bits);
        let mut full = [0u8; 64];
        for (k, s) in self.state.iter().enumerate() {
            full[8 * k..8 * k + 8].copy_from_slice(&s.to_be_bytes());
        }
        out[..self.digest].copy_from_slice(&full[..self.digest]);
    }

    fn image(&self) -> Vec<u8> {
        let mut v = vec![0u8; 208];
        let mut w = Img(&mut v);
        w.u64s(0, &self.state);
        w.u64s(64, &self.count);
        w.bytes(80, &self.buffer);
        v
    }

    fn set_image(&mut self, img: &[u8]) {
        self.state = rd_u64s(img, 0);
        self.count = rd_u64s(img, 64);
        self.buffer = rd_bytes(img, 80);
    }
}
