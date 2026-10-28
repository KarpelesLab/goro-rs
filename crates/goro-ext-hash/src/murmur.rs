//! MurmurHash3 (Austin Appleby, public domain): murmur3a is the x86
//! 32-bit variant, murmur3c the x86 128-bit one and murmur3f the x64
//! 128-bit one. Digests are printed as big-endian words.
//!
//! Streaming keeps the pending tail bytes in "carry" words: each new byte
//! is shifted in from the top of the word it belongs to, and the number of
//! pending bytes sits in the low byte of the last carry word. Serialized
//! layout observed on PHP 8.5: the hash words, the carry words and the
//! total length (32 bits). For the 128-bit variants the carry words also
//! hold the mixed block words of the last block compressed, which shows in
//! `serialize()` output.

use crate::engine::{Engine, Field, Kind, Val, clone_box, f, le32, le64};

pub(crate) const MURMUR3A_LAYOUT: &[Field] =
    &[f(Kind::U32, 1, 0), f(Kind::U32, 1, 4), f(Kind::U32, 1, 8)];
pub(crate) const MURMUR3C_LAYOUT: &[Field] =
    &[f(Kind::U32, 4, 0), f(Kind::U32, 4, 16), f(Kind::U32, 1, 32)];
pub(crate) const MURMUR3F_LAYOUT: &[Field] =
    &[f(Kind::U64, 2, 0), f(Kind::U64, 2, 16), f(Kind::U32, 1, 32)];

#[inline]
fn fmix32(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^ (h >> 16)
}

#[inline]
fn fmix64(mut k: u64) -> u64 {
    k ^= k >> 33;
    k = k.wrapping_mul(0xff51_afd7_ed55_8ccd);
    k ^= k >> 33;
    k = k.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    k ^ (k >> 33)
}

// ---------------------------------------------------------------- x86_32

const C1: u32 = 0xcc9e_2d51;
const C2: u32 = 0x1b87_3593;

#[inline]
fn mix_k32(k: u32) -> u32 {
    k.wrapping_mul(C1).rotate_left(15).wrapping_mul(C2)
}

#[inline]
fn block32(h: u32, k: u32) -> u32 {
    (h ^ mix_k32(k))
        .rotate_left(13)
        .wrapping_mul(5)
        .wrapping_add(0xe654_6b64)
}

#[derive(Clone)]
pub(crate) struct Murmur3a {
    h: u32,
    carry: u32,
    len: u32,
}

impl Murmur3a {
    pub(crate) fn new(seed: u32) -> Self {
        Murmur3a {
            h: seed,
            carry: 0,
            len: 0,
        }
    }
}

impl Engine for Murmur3a {
    fn update(&mut self, data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u32);
        let mut c = self.carry;
        let mut n = (c & 3) as usize;
        let mut h = self.h;
        let mut i = 0;
        // Complete a pending word first.
        while n != 0 && i < data.len() {
            c = (c >> 8) | ((data[i] as u32) << 24);
            n += 1;
            i += 1;
            if n == 4 {
                c = mix_k32(c);
                h = (h ^ c)
                    .rotate_left(13)
                    .wrapping_mul(5)
                    .wrapping_add(0xe654_6b64);
                n = 0;
            }
        }
        let (words, words_rest) = data[i..].as_chunks::<4>();
        for w in words {
            h = block32(h, le32(w));
        }
        for &b in words_rest {
            c = (c >> 8) | ((b as u32) << 24);
            n += 1;
        }
        self.h = h;
        self.carry = (c & !0xff) | n as u32;
    }

    fn finish(&mut self, out: &mut [u8]) {
        let mut h = self.h;
        let n = (self.len & 3) as usize;
        if n != 0 {
            let k = self.carry >> ((4 - n) * 8);
            h ^= mix_k32(k);
        }
        h ^= self.len;
        out.copy_from_slice(&fmix32(h).to_be_bytes());
    }

    clone_box!();

    fn export(&self) -> Vec<Val> {
        vec![Val::U32(self.h), Val::U32(self.carry), Val::U32(self.len)]
    }

    fn import(&mut self, v: &[Val]) -> bool {
        self.h = v[0].u32();
        self.carry = v[1].u32();
        self.len = v[2].u32();
        true
    }
}

// ---------------------------------------------------------------- x86_128

const D1: u32 = 0x239b_961b;
const D2: u32 = 0xab0e_9789;
const D3: u32 = 0x38b3_4ae5;
const D4: u32 = 0xa1e3_8b93;

#[inline]
fn mixk128(k: &[u32; 4]) -> [u32; 4] {
    [
        k[0].wrapping_mul(D1).rotate_left(15).wrapping_mul(D2),
        k[1].wrapping_mul(D2).rotate_left(16).wrapping_mul(D3),
        k[2].wrapping_mul(D3).rotate_left(17).wrapping_mul(D4),
        k[3].wrapping_mul(D4).rotate_left(18).wrapping_mul(D1),
    ]
}

#[inline]
fn block128(h: &mut [u32; 4], k: &[u32; 4]) {
    h[0] ^= k[0];
    h[0] = h[0]
        .rotate_left(19)
        .wrapping_add(h[1])
        .wrapping_mul(5)
        .wrapping_add(0x561c_cd1b);
    h[1] ^= k[1];
    h[1] = h[1]
        .rotate_left(17)
        .wrapping_add(h[2])
        .wrapping_mul(5)
        .wrapping_add(0x0bca_a747);
    h[2] ^= k[2];
    h[2] = h[2]
        .rotate_left(15)
        .wrapping_add(h[3])
        .wrapping_mul(5)
        .wrapping_add(0x96cd_1c35);
    h[3] ^= k[3];
    h[3] = h[3]
        .rotate_left(13)
        .wrapping_add(h[0])
        .wrapping_mul(5)
        .wrapping_add(0x32ac_3b17);
}

#[derive(Clone)]
pub(crate) struct Murmur3c {
    h: [u32; 4],
    c: [u32; 4],
    len: u32,
}

impl Murmur3c {
    pub(crate) fn new(seed: u32) -> Self {
        Murmur3c {
            h: [seed; 4],
            c: [0; 4],
            len: 0,
        }
    }
}

impl Engine for Murmur3c {
    fn update(&mut self, data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u32);
        let mut n = (self.c[3] & 15) as usize;
        let mut i = 0;
        while n != 0 && i < data.len() {
            let w = n / 4;
            self.c[w] = (self.c[w] >> 8) | ((data[i] as u32) << 24);
            n += 1;
            i += 1;
            if n == 16 {
                self.c = mixk128(&self.c);
                let c = self.c;
                block128(&mut self.h, &c);
                n = 0;
            }
        }
        let (blocks, blocks_rest) = data[i..].as_chunks::<16>();
        for b in blocks {
            let k = [le32(b), le32(&b[4..]), le32(&b[8..]), le32(&b[12..])];
            self.c = mixk128(&k);
            let c = self.c;
            block128(&mut self.h, &c);
        }
        for &b in blocks_rest {
            let w = n / 4;
            self.c[w] = (self.c[w] >> 8) | ((b as u32) << 24);
            n += 1;
        }
        self.c[3] = (self.c[3] & !0xff) | n as u32;
    }

    fn finish(&mut self, out: &mut [u8]) {
        let n = (self.c[3] & 15) as usize;
        let mut h = self.h;
        // Tail words: word j holds m = min(4, n - 4j) bytes at its top.
        let mut k = [0u32; 4];
        for (j, kj) in k.iter_mut().enumerate() {
            let m = n.saturating_sub(4 * j).min(4);
            if m > 0 {
                *kj = self.c[j] >> (8 * (4 - m));
            }
        }
        let mk = mixk128(&k);
        for j in (0..4).rev() {
            if n > 4 * j {
                h[j] ^= mk[j];
            }
        }
        let len = self.len;
        for x in h.iter_mut() {
            *x ^= len;
        }
        h[0] = h[0]
            .wrapping_add(h[1])
            .wrapping_add(h[2])
            .wrapping_add(h[3]);
        h[1] = h[1].wrapping_add(h[0]);
        h[2] = h[2].wrapping_add(h[0]);
        h[3] = h[3].wrapping_add(h[0]);
        for x in h.iter_mut() {
            *x = fmix32(*x);
        }
        h[0] = h[0]
            .wrapping_add(h[1])
            .wrapping_add(h[2])
            .wrapping_add(h[3]);
        h[1] = h[1].wrapping_add(h[0]);
        h[2] = h[2].wrapping_add(h[0]);
        h[3] = h[3].wrapping_add(h[0]);
        for (i, x) in h.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
        }
    }

    clone_box!();

    fn export(&self) -> Vec<Val> {
        let mut v: Vec<Val> = self.h.iter().chain(&self.c).map(|&w| Val::U32(w)).collect();
        v.push(Val::U32(self.len));
        v
    }

    fn import(&mut self, v: &[Val]) -> bool {
        for i in 0..4 {
            self.h[i] = v[i].u32();
            self.c[i] = v[4 + i].u32();
        }
        self.len = v[8].u32();
        true
    }
}

// ---------------------------------------------------------------- x64_128

const E1: u64 = 0x87c3_7b91_1142_53d5;
const E2: u64 = 0x4cf5_ad43_2745_937f;

#[inline]
fn mixk64(k: &[u64; 2]) -> [u64; 2] {
    [
        k[0].wrapping_mul(E1).rotate_left(31).wrapping_mul(E2),
        k[1].wrapping_mul(E2).rotate_left(33).wrapping_mul(E1),
    ]
}

#[inline]
fn block64(h: &mut [u64; 2], k: &[u64; 2]) {
    h[0] ^= k[0];
    h[0] = h[0]
        .rotate_left(27)
        .wrapping_add(h[1])
        .wrapping_mul(5)
        .wrapping_add(0x52dc_e729);
    h[1] ^= k[1];
    h[1] = h[1]
        .rotate_left(31)
        .wrapping_add(h[0])
        .wrapping_mul(5)
        .wrapping_add(0x3849_5ab5);
}

#[derive(Clone)]
pub(crate) struct Murmur3f {
    h: [u64; 2],
    c: [u64; 2],
    len: u32,
}

impl Murmur3f {
    pub(crate) fn new(seed: u64) -> Self {
        Murmur3f {
            h: [seed; 2],
            c: [0; 2],
            len: 0,
        }
    }
}

impl Engine for Murmur3f {
    fn update(&mut self, data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u32);
        let mut n = (self.c[1] & 15) as usize;
        let mut i = 0;
        while n != 0 && i < data.len() {
            let w = n / 8;
            self.c[w] = (self.c[w] >> 8) | ((data[i] as u64) << 56);
            n += 1;
            i += 1;
            if n == 16 {
                self.c = mixk64(&self.c);
                let c = self.c;
                block64(&mut self.h, &c);
                n = 0;
            }
        }
        let (blocks, blocks_rest) = data[i..].as_chunks::<16>();
        for b in blocks {
            let k = [le64(b), le64(&b[8..])];
            self.c = mixk64(&k);
            let c = self.c;
            block64(&mut self.h, &c);
        }
        for &b in blocks_rest {
            let w = n / 8;
            self.c[w] = (self.c[w] >> 8) | ((b as u64) << 56);
            n += 1;
        }
        self.c[1] = (self.c[1] & !0xff) | n as u64;
    }

    fn finish(&mut self, out: &mut [u8]) {
        let n = (self.c[1] & 15) as usize;
        let mut h = self.h;
        let mut k = [0u64; 2];
        for (j, kj) in k.iter_mut().enumerate() {
            let m = n.saturating_sub(8 * j).min(8);
            if m > 0 {
                *kj = self.c[j] >> (8 * (8 - m));
            }
        }
        let mk = mixk64(&k);
        if n > 8 {
            h[1] ^= mk[1];
        }
        if n > 0 {
            h[0] ^= mk[0];
        }
        let len = self.len as u64;
        h[0] ^= len;
        h[1] ^= len;
        h[0] = h[0].wrapping_add(h[1]);
        h[1] = h[1].wrapping_add(h[0]);
        h[0] = fmix64(h[0]);
        h[1] = fmix64(h[1]);
        h[0] = h[0].wrapping_add(h[1]);
        h[1] = h[1].wrapping_add(h[0]);
        out[..8].copy_from_slice(&h[0].to_be_bytes());
        out[8..].copy_from_slice(&h[1].to_be_bytes());
    }

    clone_box!();

    fn export(&self) -> Vec<Val> {
        vec![
            Val::U64(self.h[0]),
            Val::U64(self.h[1]),
            Val::U64(self.c[0]),
            Val::U64(self.c[1]),
            Val::U32(self.len),
        ]
    }

    fn import(&mut self, v: &[Val]) -> bool {
        self.h = [v[0].u64(), v[1].u64()];
        self.c = [v[2].u64(), v[3].u64()];
        self.len = v[4].u32();
        true
    }
}
