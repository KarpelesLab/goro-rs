//! xxHash: XXH32, XXH64, XXH3-64 and XXH3-128, from the xxHash
//! specification v0.2.0 (github.com/Cyan4973/xxHash, doc/xxhash_spec.md).
//! Digests use the canonical big-endian form.
//!
//! XXH32/XXH64 serialized layouts observed on PHP 8.5: total length, the
//! "large" flag (XXH32 only), the four accumulators, the pending stripe
//! memory, its fill count (which must stay below the stripe size) and
//! reserved words. XXH3 contexts cannot be serialized.

use crate::engine::{Engine, Field, Kind, Val, clone_box, f, le32, le64};

pub(crate) const XXH32_LAYOUT: &[Field] = &[
    f(Kind::U32, 1, 0),
    f(Kind::U32, 1, 4),
    f(Kind::U32, 4, 8),
    f(Kind::U32, 4, 24),
    f(Kind::U32, 1, 40),
    f(Kind::U32, 1, 44),
];
pub(crate) const XXH64_LAYOUT: &[Field] = &[
    f(Kind::U64, 1, 0),
    f(Kind::U64, 4, 8),
    f(Kind::U64, 4, 40),
    f(Kind::U32, 1, 72),
    f(Kind::U32, 1, 76),
    f(Kind::U64, 1, 80),
];

const P32_1: u32 = 0x9e37_79b1;
const P32_2: u32 = 0x85eb_ca77;
const P32_3: u32 = 0xc2b2_ae3d;
const P32_4: u32 = 0x27d4_eb2f;
const P32_5: u32 = 0x1656_67b1;
const P64_1: u64 = 0x9e37_79b1_85eb_ca87;
const P64_2: u64 = 0xc2b2_ae3d_27d4_eb4f;
const P64_3: u64 = 0x1656_67b1_9e37_79f9;
const P64_4: u64 = 0x85eb_ca77_c2b2_ae63;
const P64_5: u64 = 0x27d4_eb2f_1656_67c5;
const PMX1: u64 = 0x1656_6791_9e37_79f9;
const PMX2: u64 = 0x9fb2_1c65_1e98_df25;

// ---------------------------------------------------------------- XXH32

#[inline]
fn round32(acc: u32, lane: u32) -> u32 {
    acc.wrapping_add(lane.wrapping_mul(P32_2))
        .rotate_left(13)
        .wrapping_mul(P32_1)
}

#[derive(Clone)]
pub(crate) struct Xxh32 {
    total: u32,
    large: u32,
    v: [u32; 4],
    mem: [u8; 16],
    memsize: usize,
    reserved: u32,
}

impl Xxh32 {
    pub(crate) fn new(seed: u32) -> Self {
        Xxh32 {
            total: 0,
            large: 0,
            v: [
                seed.wrapping_add(P32_1).wrapping_add(P32_2),
                seed.wrapping_add(P32_2),
                seed,
                seed.wrapping_sub(P32_1),
            ],
            mem: [0; 16],
            memsize: 0,
            reserved: 0,
        }
    }

    #[inline]
    fn stripe(v: &mut [u32; 4], s: &[u8]) {
        for (i, x) in v.iter_mut().enumerate() {
            *x = round32(*x, le32(&s[i * 4..]));
        }
    }
}

impl Engine for Xxh32 {
    fn update(&mut self, mut data: &[u8]) {
        self.total = self.total.wrapping_add(data.len() as u32);
        self.large |= ((data.len() >= 16) | (self.total >= 16)) as u32;
        if self.memsize + data.len() < 16 {
            self.mem[self.memsize..self.memsize + data.len()].copy_from_slice(data);
            self.memsize += data.len();
            return;
        }
        if self.memsize != 0 {
            let take = 16 - self.memsize;
            self.mem[self.memsize..].copy_from_slice(&data[..take]);
            let m = self.mem;
            Self::stripe(&mut self.v, &m);
            data = &data[take..];
            self.memsize = 0;
        }
        let (stripes, stripes_rest) = data.as_chunks::<16>();
        for s in stripes {
            Self::stripe(&mut self.v, s);
        }
        let rest = stripes_rest;
        self.mem[..rest.len()].copy_from_slice(rest);
        self.memsize = rest.len();
    }

    fn finish(&mut self, out: &mut [u8]) {
        let v = self.v;
        let mut h = if self.large != 0 {
            v[0].rotate_left(1)
                .wrapping_add(v[1].rotate_left(7))
                .wrapping_add(v[2].rotate_left(12))
                .wrapping_add(v[3].rotate_left(18))
        } else {
            v[2].wrapping_add(P32_5)
        };
        h = h.wrapping_add(self.total);
        let mut rest = &self.mem[..self.memsize];
        while rest.len() >= 4 {
            h = h
                .wrapping_add(le32(rest).wrapping_mul(P32_3))
                .rotate_left(17)
                .wrapping_mul(P32_4);
            rest = &rest[4..];
        }
        for &b in rest {
            h = h
                .wrapping_add((b as u32).wrapping_mul(P32_5))
                .rotate_left(11)
                .wrapping_mul(P32_1);
        }
        h ^= h >> 15;
        h = h.wrapping_mul(P32_2);
        h ^= h >> 13;
        h = h.wrapping_mul(P32_3);
        h ^= h >> 16;
        out.copy_from_slice(&h.to_be_bytes());
    }

    clone_box!();

    fn export(&self) -> Vec<Val> {
        let mut v = vec![Val::U32(self.total), Val::U32(self.large)];
        v.extend(self.v.iter().map(|&x| Val::U32(x)));
        for i in 0..4 {
            v.push(Val::U32(le32(&self.mem[i * 4..])));
        }
        v.push(Val::U32(self.memsize as u32));
        v.push(Val::U32(self.reserved));
        v
    }

    fn import(&mut self, v: &[Val]) -> bool {
        let memsize = v[10].u32() as usize;
        if memsize >= 16 {
            return false;
        }
        self.total = v[0].u32();
        self.large = v[1].u32();
        for i in 0..4 {
            self.v[i] = v[2 + i].u32();
            self.mem[i * 4..i * 4 + 4].copy_from_slice(&v[6 + i].u32().to_le_bytes());
        }
        self.memsize = memsize;
        self.reserved = v[11].u32();
        true
    }
}

// ---------------------------------------------------------------- XXH64

#[inline]
fn round64(acc: u64, lane: u64) -> u64 {
    acc.wrapping_add(lane.wrapping_mul(P64_2))
        .rotate_left(31)
        .wrapping_mul(P64_1)
}

#[inline]
fn merge64(acc: u64, v: u64) -> u64 {
    (acc ^ round64(0, v))
        .wrapping_mul(P64_1)
        .wrapping_add(P64_4)
}

#[inline]
fn avalanche_xxh64(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(P64_2);
    h ^= h >> 29;
    h = h.wrapping_mul(P64_3);
    h ^ (h >> 32)
}

#[derive(Clone)]
pub(crate) struct Xxh64 {
    total: u64,
    v: [u64; 4],
    mem: [u8; 32],
    memsize: usize,
    reserved32: u32,
    reserved64: u64,
}

impl Xxh64 {
    pub(crate) fn new(seed: u64) -> Self {
        Xxh64 {
            total: 0,
            v: [
                seed.wrapping_add(P64_1).wrapping_add(P64_2),
                seed.wrapping_add(P64_2),
                seed,
                seed.wrapping_sub(P64_1),
            ],
            mem: [0; 32],
            memsize: 0,
            reserved32: 0,
            reserved64: 0,
        }
    }

    #[inline]
    fn stripe(v: &mut [u64; 4], s: &[u8]) {
        for (i, x) in v.iter_mut().enumerate() {
            *x = round64(*x, le64(&s[i * 8..]));
        }
    }
}

impl Engine for Xxh64 {
    fn update(&mut self, mut data: &[u8]) {
        self.total = self.total.wrapping_add(data.len() as u64);
        if self.memsize + data.len() < 32 {
            self.mem[self.memsize..self.memsize + data.len()].copy_from_slice(data);
            self.memsize += data.len();
            return;
        }
        if self.memsize != 0 {
            let take = 32 - self.memsize;
            self.mem[self.memsize..].copy_from_slice(&data[..take]);
            let m = self.mem;
            Self::stripe(&mut self.v, &m);
            data = &data[take..];
            self.memsize = 0;
        }
        let (stripes, stripes_rest) = data.as_chunks::<32>();
        for s in stripes {
            Self::stripe(&mut self.v, s);
        }
        let rest = stripes_rest;
        self.mem[..rest.len()].copy_from_slice(rest);
        self.memsize = rest.len();
    }

    fn finish(&mut self, out: &mut [u8]) {
        let v = self.v;
        let mut h = if self.total >= 32 {
            let mut h = v[0]
                .rotate_left(1)
                .wrapping_add(v[1].rotate_left(7))
                .wrapping_add(v[2].rotate_left(12))
                .wrapping_add(v[3].rotate_left(18));
            for &x in &v {
                h = merge64(h, x);
            }
            h
        } else {
            v[2].wrapping_add(P64_5)
        };
        h = h.wrapping_add(self.total);
        let mut rest = &self.mem[..self.memsize];
        while rest.len() >= 8 {
            h = (h ^ round64(0, le64(rest)))
                .rotate_left(27)
                .wrapping_mul(P64_1)
                .wrapping_add(P64_4);
            rest = &rest[8..];
        }
        if rest.len() >= 4 {
            h = (h ^ (le32(rest) as u64).wrapping_mul(P64_1))
                .rotate_left(23)
                .wrapping_mul(P64_2)
                .wrapping_add(P64_3);
            rest = &rest[4..];
        }
        for &b in rest {
            h = (h ^ (b as u64).wrapping_mul(P64_5))
                .rotate_left(11)
                .wrapping_mul(P64_1);
        }
        out.copy_from_slice(&avalanche_xxh64(h).to_be_bytes());
    }

    clone_box!();

    fn export(&self) -> Vec<Val> {
        let mut v = vec![Val::U64(self.total)];
        v.extend(self.v.iter().map(|&x| Val::U64(x)));
        for i in 0..4 {
            v.push(Val::U64(le64(&self.mem[i * 8..])));
        }
        v.push(Val::U32(self.memsize as u32));
        v.push(Val::U32(self.reserved32));
        v.push(Val::U64(self.reserved64));
        v
    }

    fn import(&mut self, v: &[Val]) -> bool {
        let memsize = v[9].u32() as usize;
        if memsize >= 32 {
            return false;
        }
        self.total = v[0].u64();
        for i in 0..4 {
            self.v[i] = v[1 + i].u64();
            self.mem[i * 8..i * 8 + 8].copy_from_slice(&v[5 + i].u64().to_le_bytes());
        }
        self.memsize = memsize;
        self.reserved32 = v[10].u32();
        self.reserved64 = v[11].u64();
        true
    }
}

// ---------------------------------------------------------------- XXH3

/// The default 192-byte secret of the specification.
#[rustfmt::skip]
pub(crate) const DEFAULT_SECRET: [u8; 192] = [
    0xb8, 0xfe, 0x6c, 0x39, 0x23, 0xa4, 0x4b, 0xbe, 0x7c, 0x01, 0x81, 0x2c, 0xf7, 0x21, 0xad, 0x1c,
    0xde, 0xd4, 0x6d, 0xe9, 0x83, 0x90, 0x97, 0xdb, 0x72, 0x40, 0xa4, 0xa4, 0xb7, 0xb3, 0x67, 0x1f,
    0xcb, 0x79, 0xe6, 0x4e, 0xcc, 0xc0, 0xe5, 0x78, 0x82, 0x5a, 0xd0, 0x7d, 0xcc, 0xff, 0x72, 0x21,
    0xb8, 0x08, 0x46, 0x74, 0xf7, 0x43, 0x24, 0x8e, 0xe0, 0x35, 0x90, 0xe6, 0x81, 0x3a, 0x26, 0x4c,
    0x3c, 0x28, 0x52, 0xbb, 0x91, 0xc3, 0x00, 0xcb, 0x88, 0xd0, 0x65, 0x8b, 0x1b, 0x53, 0x2e, 0xa3,
    0x71, 0x64, 0x48, 0x97, 0xa2, 0x0d, 0xf9, 0x4e, 0x38, 0x19, 0xef, 0x46, 0xa9, 0xde, 0xac, 0xd8,
    0xa8, 0xfa, 0x76, 0x3f, 0xe3, 0x9c, 0x34, 0x3f, 0xf9, 0xdc, 0xbb, 0xc7, 0xc7, 0x0b, 0x4f, 0x1d,
    0x8a, 0x51, 0xe0, 0x4b, 0xcd, 0xb4, 0x59, 0x31, 0xc8, 0x9f, 0x7e, 0xc9, 0xd9, 0x78, 0x73, 0x64,
    0xea, 0xc5, 0xac, 0x83, 0x34, 0xd3, 0xeb, 0xc3, 0xc5, 0x81, 0xa0, 0xff, 0xfa, 0x13, 0x63, 0xeb,
    0x17, 0x0d, 0xdd, 0x51, 0xb7, 0xf0, 0xda, 0x49, 0xd3, 0x16, 0x55, 0x26, 0x29, 0xd4, 0x68, 0x9e,
    0x2b, 0x16, 0xbe, 0x58, 0x7d, 0x47, 0xa1, 0xfc, 0x8f, 0xf8, 0xb8, 0xd1, 0x7a, 0xd0, 0x31, 0xce,
    0x45, 0xcb, 0x3a, 0x8f, 0x95, 0x16, 0x04, 0x28, 0xaf, 0xd7, 0xfb, 0xca, 0xbb, 0x4b, 0x40, 0x7e,
];

/// The smallest secret XXH3 accepts.
pub const XXH3_SECRET_MIN: usize = 136;
/// PHP keeps at most this many bytes of a custom secret (observed).
pub const XXH3_SECRET_MAX: usize = 256;

#[inline]
fn avalanche3(mut x: u64) -> u64 {
    x ^= x >> 37;
    x = x.wrapping_mul(PMX1);
    x ^ (x >> 32)
}

#[inline]
fn mul128(a: u64, b: u64) -> (u64, u64) {
    let r = (a as u128) * (b as u128);
    (r as u64, (r >> 64) as u64)
}

#[inline]
fn mul_fold(a: u64, b: u64) -> u64 {
    let (lo, hi) = mul128(a, b);
    lo ^ hi
}

#[inline]
fn mix16(data: &[u8], secret: &[u8], seed: u64) -> u64 {
    mul_fold(
        le64(data) ^ le64(secret).wrapping_add(seed),
        le64(&data[8..]) ^ le64(&secret[8..]).wrapping_sub(seed),
    )
}

/// XXH3 with 64 or 128 bits of output.
#[derive(Clone)]
pub(crate) struct Xxh3 {
    wide: bool,
    seed: u64,
    /// The secret in use: default, custom or derived from the seed.
    secret: Vec<u8>,
    /// Whether a custom secret was given (seed then stays 0).
    custom: bool,
    total: u64,
    /// Bytes not yet consumed: the whole input while it is at most 240
    /// bytes, then the 1 to 64 bytes after the last accumulated stripe.
    buf: Vec<u8>,
    /// The 64 bytes before `buf` once stripes are being accumulated.
    last: [u8; 64],
    acc: [u64; 8],
    stripe_in_block: usize,
}

const ACC_INIT: [u64; 8] = [
    P32_3 as u64,
    P64_1,
    P64_2,
    P64_3,
    P64_4,
    P32_2 as u64,
    P64_5,
    P32_1 as u64,
];

impl Xxh3 {
    /// A seeded XXH3 (seed 0 is the plain one).
    pub(crate) fn with_seed(wide: bool, seed: u64) -> Self {
        let mut secret = DEFAULT_SECRET.to_vec();
        if seed != 0 {
            for i in 0..12 {
                let a = le64(&secret[i * 16..]).wrapping_add(seed);
                let b = le64(&secret[i * 16 + 8..]).wrapping_sub(seed);
                secret[i * 16..i * 16 + 8].copy_from_slice(&a.to_le_bytes());
                secret[i * 16 + 8..i * 16 + 16].copy_from_slice(&b.to_le_bytes());
            }
        }
        Self::make(wide, seed, secret, false)
    }

    /// XXH3 with a custom secret (at least 136 bytes).
    pub(crate) fn with_secret(wide: bool, secret: &[u8]) -> Self {
        Self::make(wide, 0, secret.to_vec(), true)
    }

    fn make(wide: bool, seed: u64, secret: Vec<u8>, custom: bool) -> Self {
        Xxh3 {
            wide,
            seed,
            secret,
            custom,
            total: 0,
            buf: Vec::new(),
            last: [0; 64],
            acc: ACC_INIT,
            stripe_in_block: 0,
        }
    }

    /// The secret used for inputs of at most 240 bytes: the default one
    /// for seeded hashing, else the custom one.
    fn short_secret(&self) -> &[u8] {
        if self.custom {
            &self.secret
        } else {
            &DEFAULT_SECRET
        }
    }

    /// Accumulates every stripe of `data` that has at least one byte after
    /// it; returns the unconsumed tail (1 to 64 bytes when `data` is not
    /// empty).
    fn consume<'a>(&mut self, data: &'a [u8]) -> &'a [u8] {
        if data.is_empty() {
            return data;
        }
        let n = (data.len() - 1) / 64;
        self.stripe_in_block = crate::xxh3_lanes::stripes(
            &mut self.acc,
            &data[..n * 64],
            &self.secret,
            self.stripe_in_block,
        );
        if n > 0 {
            self.last.copy_from_slice(&data[(n - 1) * 64..n * 64]);
        }
        &data[n * 64..]
    }

    fn short_hash(&self, input: &[u8]) -> (u64, u64) {
        let secret = self.short_secret();
        let seed = self.seed;
        let len = input.len();
        if len <= 16 {
            return small(input, secret, seed, self.wide);
        }
        if !self.wide {
            let mut acc = (len as u64).wrapping_mul(P64_1);
            if len <= 128 {
                let rounds = ((len - 1) >> 5) + 1;
                for i in (0..rounds).rev() {
                    acc = acc.wrapping_add(mix16(&input[i * 16..], &secret[i * 32..], seed));
                    acc = acc.wrapping_add(mix16(
                        &input[len - i * 16 - 16..],
                        &secret[i * 32 + 16..],
                        seed,
                    ));
                }
            } else {
                for i in 0..8 {
                    acc = acc.wrapping_add(mix16(&input[i * 16..], &secret[i * 16..], seed));
                }
                acc = avalanche3(acc);
                for i in 8..len / 16 {
                    acc = acc.wrapping_add(mix16(
                        &input[i * 16..],
                        &secret[(i - 8) * 16 + 3..],
                        seed,
                    ));
                }
                acc = acc.wrapping_add(mix16(&input[len - 16..], &secret[119..], seed));
            }
            return (avalanche3(acc), 0);
        }
        let mut acc = [(len as u64).wrapping_mul(P64_1), 0u64];
        let mix2 = |acc: &mut [u64; 2], d1: &[u8], d2: &[u8], so: usize, seed: u64| {
            acc[0] = acc[0].wrapping_add(mix16(d1, &secret[so..], seed));
            acc[1] = acc[1].wrapping_add(mix16(d2, &secret[so + 16..], seed));
            acc[0] ^= le64(d2).wrapping_add(le64(&d2[8..]));
            acc[1] ^= le64(d1).wrapping_add(le64(&d1[8..]));
        };
        if len <= 128 {
            let rounds = ((len - 1) >> 5) + 1;
            for i in (0..rounds).rev() {
                mix2(
                    &mut acc,
                    &input[i * 16..],
                    &input[len - i * 16 - 16..],
                    i * 32,
                    seed,
                );
            }
        } else {
            for i in 0..4 {
                mix2(
                    &mut acc,
                    &input[i * 32..],
                    &input[i * 32 + 16..],
                    i * 32,
                    seed,
                );
            }
            acc[0] = avalanche3(acc[0]);
            acc[1] = avalanche3(acc[1]);
            for i in 4..len / 32 {
                mix2(
                    &mut acc,
                    &input[i * 32..],
                    &input[i * 32 + 16..],
                    (i - 4) * 32 + 3,
                    seed,
                );
            }
            mix2(
                &mut acc,
                &input[len - 16..],
                &input[len - 32..],
                103,
                0u64.wrapping_sub(seed),
            );
        }
        let low = acc[0].wrapping_add(acc[1]);
        let high = acc[0]
            .wrapping_mul(P64_1)
            .wrapping_add(acc[1].wrapping_mul(P64_4))
            .wrapping_add((len as u64).wrapping_sub(seed).wrapping_mul(P64_2));
        (avalanche3(low), 0u64.wrapping_sub(avalanche3(high)))
    }

    fn final_merge(acc: &[u64; 8], init: u64, secret: &[u8]) -> u64 {
        let mut r = init;
        for i in 0..4 {
            r = r.wrapping_add(mul_fold(
                acc[2 * i] ^ le64(&secret[16 * i..]),
                acc[2 * i + 1] ^ le64(&secret[16 * i + 8..]),
            ));
        }
        avalanche3(r)
    }
}

fn small(input: &[u8], secret: &[u8], seed: u64, wide: bool) -> (u64, u64) {
    let len = input.len();
    if len == 0 {
        let lo = avalanche_xxh64(seed ^ le64(&secret[56..]) ^ le64(&secret[64..]));
        if !wide {
            return (lo, 0);
        }
        return (
            avalanche_xxh64(seed ^ le64(&secret[64..]) ^ le64(&secret[72..])),
            avalanche_xxh64(seed ^ le64(&secret[80..]) ^ le64(&secret[88..])),
        );
    }
    if len <= 3 {
        let combined = (input[len - 1] as u32)
            | ((len as u32) << 8)
            | ((input[0] as u32) << 16)
            | ((input[len >> 1] as u32) << 24);
        let lo = ((le32(secret) ^ le32(&secret[4..])) as u64).wrapping_add(seed) ^ combined as u64;
        if !wide {
            return (avalanche_xxh64(lo), 0);
        }
        let hi = ((le32(&secret[8..]) ^ le32(&secret[12..])) as u64).wrapping_sub(seed)
            ^ (combined.swap_bytes().rotate_left(13)) as u64;
        return (avalanche_xxh64(lo), avalanche_xxh64(hi));
    }
    if len <= 8 {
        let first = le32(input);
        let last = le32(&input[len - 4..]);
        let mseed = seed ^ (((seed as u32).swap_bytes() as u64) << 32);
        if !wide {
            let combined = last as u64 | ((first as u64) << 32);
            let mut v = (le64(&secret[8..]) ^ le64(&secret[16..])).wrapping_sub(mseed) ^ combined;
            v ^= v.rotate_left(49) ^ v.rotate_left(24);
            v = v.wrapping_mul(PMX2);
            v ^= (v >> 35).wrapping_add(len as u64);
            v = v.wrapping_mul(PMX2);
            v ^= v >> 28;
            return (v, 0);
        }
        let combined = first as u64 | ((last as u64) << 32);
        let v = (le64(&secret[16..]) ^ le64(&secret[24..])).wrapping_add(mseed) ^ combined;
        let (mut lo, mut hi) = mul128(v, P64_1.wrapping_add((len as u64) << 2));
        hi = hi.wrapping_add(lo << 1);
        lo ^= hi >> 3;
        lo ^= lo >> 35;
        lo = lo.wrapping_mul(PMX2);
        lo ^= lo >> 28;
        return (lo, avalanche3(hi));
    }
    let first = le64(input);
    let last = le64(&input[len - 8..]);
    if !wide {
        let lo = (le64(&secret[24..]) ^ le64(&secret[32..])).wrapping_add(seed) ^ first;
        let hi = (le64(&secret[40..]) ^ le64(&secret[48..])).wrapping_sub(seed) ^ last;
        let v = (len as u64)
            .wrapping_add(lo.swap_bytes())
            .wrapping_add(hi)
            .wrapping_add(mul_fold(lo, hi));
        return (avalanche3(v), 0);
    }
    let val1 = (le64(&secret[32..]) ^ le64(&secret[40..])).wrapping_sub(seed) ^ first ^ last;
    let val2 = (le64(&secret[48..]) ^ le64(&secret[56..])).wrapping_add(seed) ^ last;
    let (mlo, mhi) = mul128(val1, P64_1);
    let mut low = mlo.wrapping_add(((len - 1) as u64) << 54);
    let high = mhi
        .wrapping_add((val2 >> 32) << 32)
        .wrapping_add((val2 & 0xffff_ffff).wrapping_mul(P32_2 as u64));
    low ^= high.swap_bytes();
    let (l2, h2) = mul128(low, P64_2);
    let high = h2.wrapping_add(high.wrapping_mul(P64_2));
    (avalanche3(l2), avalanche3(high))
}

impl Engine for Xxh3 {
    fn update(&mut self, data: &[u8]) {
        let before = self.total;
        self.total = self.total.wrapping_add(data.len() as u64);
        if self.total <= 240 {
            self.buf.extend_from_slice(data);
            return;
        }
        let mut data = data;
        if before <= 240 {
            // Switch to stripe processing: everything so far is buffered.
            let mut all = std::mem::take(&mut self.buf);
            all.extend_from_slice(data);
            let rest = self.consume(&all).to_vec();
            self.buf = rest;
            return;
        }
        if !self.buf.is_empty() {
            let need = 64 - self.buf.len();
            if data.len() <= need {
                self.buf.extend_from_slice(data);
                return;
            }
            self.buf.extend_from_slice(&data[..need]);
            data = &data[need..];
            let b = std::mem::take(&mut self.buf);
            // `b` is one full stripe and more input follows.
            self.stripe_in_block =
                crate::xxh3_lanes::stripes(&mut self.acc, &b, &self.secret, self.stripe_in_block);
            self.last.copy_from_slice(&b);
            self.buf = b;
            self.buf.clear();
        }
        let rest = self.consume(data);
        self.buf.extend_from_slice(rest);
    }

    fn finish(&mut self, out: &mut [u8]) {
        let (lo, hi) = if self.total <= 240 {
            self.short_hash(&self.buf.clone())
        } else {
            let mut tail = [0u8; 64];
            let n = self.buf.len();
            tail[..64 - n].copy_from_slice(&self.last[n..]);
            tail[64 - n..].copy_from_slice(&self.buf);
            let mut acc = self.acc;
            let sl = self.secret.len();
            crate::xxh3_lanes::accumulate(&mut acc, &tail, &self.secret[sl - 71..]);
            let lo = Self::final_merge(&acc, self.total.wrapping_mul(P64_1), &self.secret[11..]);
            let hi = if self.wide {
                Self::final_merge(
                    &acc,
                    !self.total.wrapping_mul(P64_2),
                    &self.secret[sl - 75..],
                )
            } else {
                0
            };
            (lo, hi)
        };
        if self.wide {
            out[..8].copy_from_slice(&hi.to_be_bytes());
            out[8..].copy_from_slice(&lo.to_be_bytes());
        } else {
            out.copy_from_slice(&lo.to_be_bytes());
        }
    }

    clone_box!();

    fn export(&self) -> Vec<Val> {
        Vec::new()
    }

    fn import(&mut self, _v: &[Val]) -> bool {
        false
    }
}
