//! MurmurHash3 (ext/hash/hash_murmur.c on top of PMurHash / PMurHash128,
//! the UNALIGNED_SAFE code paths used on x86).

use crate::ops::{Ctx, Img, InitArgs, rd_u32, rd_u32s, rd_u64s};
use goro_core::value::Value;

const SEED_DEPRECATION: &str = "Passing a seed of a type other than int is deprecated because it is the same as setting the seed to 0";

/// The "seed" option: Some(seed) when an int, None (after a deprecation for
/// other types) otherwise.
fn seed_option(args: Option<&mut InitArgs>) -> Option<i64> {
    let a = args?;
    match a.find("seed")? {
        Value::Long(n) => Some(n),
        _ => {
            a.deprecated(SEED_DEPRECATION);
            None
        }
    }
}

// ---------------------------------------------------------------- murmur3a

const KC1: u32 = 0xcc9e2d51;
const KC2: u32 = 0x1b873593;

#[inline(always)]
fn doblock32(h1: &mut u32, k1: &mut u32) {
    *k1 = k1.wrapping_mul(KC1);
    *k1 = k1.rotate_left(15);
    *k1 = k1.wrapping_mul(KC2);
    *h1 ^= *k1;
    *h1 = h1.rotate_left(13);
    *h1 = h1.wrapping_mul(5).wrapping_add(0xe6546b64);
}

#[derive(Clone)]
pub struct Murmur3a {
    h: u32,
    carry: u32,
    len: u32,
}

pub fn m3a_init(args: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    let h = seed_option(args).unwrap_or(0) as u32;
    Box::new(Murmur3a {
        h,
        carry: 0,
        len: 0,
    })
}

fn pmurhash32_process(ph1: &mut u32, pcarry: &mut u32, key: &[u8]) {
    let mut h1 = *ph1;
    let mut c = *pcarry;
    let mut n = (c & 3) as usize;
    let mut ptr = 0usize;
    let mut len = key.len() as i32 as i64;
    macro_rules! dobytes {
        ($cnt:expr) => {
            let mut cnt = $cnt;
            while cnt > 0 {
                cnt -= 1;
                c = (c >> 8) | ((key[ptr] as u32) << 24);
                ptr += 1;
                n += 1;
                if n == 4 {
                    doblock32(&mut h1, &mut c);
                    n = 0;
                }
            }
        };
    }
    let i = ((4 - n) & 3) as i64;
    if i != 0 && i <= len {
        dobytes!(i);
        len -= i;
    }
    let end = ptr + (len & !3) as usize;
    while ptr < end {
        let mut k1 = u32::from_le_bytes(key[ptr..ptr + 4].try_into().unwrap());
        doblock32(&mut h1, &mut k1);
        ptr += 4;
    }
    len -= len & !3;
    dobytes!(len);
    *ph1 = h1;
    *pcarry = (c & !0xff) | n as u32;
}

fn pmurhash32_result(mut h: u32, carry: u32, total_length: u32) -> u32 {
    let n = carry & 3;
    if n != 0 {
        let mut k1 = carry >> ((4 - n) * 8);
        k1 = k1.wrapping_mul(KC1);
        k1 = k1.rotate_left(15);
        k1 = k1.wrapping_mul(KC2);
        h ^= k1;
    }
    h ^= total_length;
    fmix32(h)
}

#[inline(always)]
fn fmix32(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x85ebca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2ae35);
    h ^= h >> 16;
    h
}

#[inline(always)]
fn fmix64(mut k: u64) -> u64 {
    k ^= k >> 33;
    k = k.wrapping_mul(0xff51afd7ed558ccd);
    k ^= k >> 33;
    k = k.wrapping_mul(0xc4ceb9fe1a85ec53);
    k ^= k >> 33;
    k
}

impl Ctx for Murmur3a {
    fn update(&mut self, data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u32);
        pmurhash32_process(&mut self.h, &mut self.carry, data);
    }

    fn finish(&mut self, out: &mut [u8]) {
        self.h = pmurhash32_result(self.h, self.carry, self.len);
        out[..4].copy_from_slice(&self.h.to_be_bytes());
    }

    fn image(&self) -> Vec<u8> {
        let mut v = vec![0u8; 12];
        Img(&mut v).u32s(0, &[self.h, self.carry, self.len]);
        v
    }

    fn set_image(&mut self, img: &[u8]) {
        self.h = rd_u32(img, 0);
        self.carry = rd_u32(img, 4);
        self.len = rd_u32(img, 8);
    }
}

// ---------------------------------------------------------------- murmur3c

const C1: u32 = 0x239b961b;
const C2: u32 = 0xab0e9789;
const C3: u32 = 0x38b34ae5;
const C4: u32 = 0xa1e38b93;

#[inline(always)]
fn doblock128x86(h: &mut [u32; 4], k: &mut [u32; 4]) {
    k[0] = k[0].wrapping_mul(C1);
    k[0] = k[0].rotate_left(15);
    k[0] = k[0].wrapping_mul(C2);
    h[0] ^= k[0];
    h[0] = h[0].rotate_left(19);
    h[0] = h[0].wrapping_add(h[1]);
    h[0] = h[0].wrapping_mul(5).wrapping_add(0x561ccd1b);
    k[1] = k[1].wrapping_mul(C2);
    k[1] = k[1].rotate_left(16);
    k[1] = k[1].wrapping_mul(C3);
    h[1] ^= k[1];
    h[1] = h[1].rotate_left(17);
    h[1] = h[1].wrapping_add(h[2]);
    h[1] = h[1].wrapping_mul(5).wrapping_add(0x0bcaa747);
    k[2] = k[2].wrapping_mul(C3);
    k[2] = k[2].rotate_left(17);
    k[2] = k[2].wrapping_mul(C4);
    h[2] ^= k[2];
    h[2] = h[2].rotate_left(15);
    h[2] = h[2].wrapping_add(h[3]);
    h[2] = h[2].wrapping_mul(5).wrapping_add(0x96cd1c35);
    k[3] = k[3].wrapping_mul(C4);
    k[3] = k[3].rotate_left(18);
    k[3] = k[3].wrapping_mul(C1);
    h[3] ^= k[3];
    h[3] = h[3].rotate_left(13);
    h[3] = h[3].wrapping_add(h[0]);
    h[3] = h[3].wrapping_mul(5).wrapping_add(0x32ac3b17);
}

#[derive(Clone)]
pub struct Murmur3c {
    h: [u32; 4],
    carry: [u32; 4],
    len: u32,
}

pub fn m3c_init(args: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    let s = seed_option(args).unwrap_or(0) as u32;
    Box::new(Murmur3c {
        h: [s; 4],
        carry: [0; 4],
        len: 0,
    })
}

fn pmurhash128x86_process(ph: &mut [u32; 4], pcarry: &mut [u32; 4], key: &[u8]) {
    let mut h = *ph;
    let mut k = *pcarry;
    let mut n = (k[3] & 15) as usize;
    let mut ptr = 0usize;
    let mut len = key.len() as i32 as i64;
    macro_rules! dobytes {
        ($cnt:expr) => {
            let mut cnt = $cnt;
            while cnt > 0 {
                cnt -= 1;
                let byte = (key[ptr] as u32) << 24;
                ptr += 1;
                match n {
                    0..=3 => {
                        k[0] = (k[0] >> 8) | byte;
                        n += 1;
                    }
                    4..=7 => {
                        k[1] = (k[1] >> 8) | byte;
                        n += 1;
                    }
                    8..=11 => {
                        k[2] = (k[2] >> 8) | byte;
                        n += 1;
                    }
                    12..=14 => {
                        k[3] = (k[3] >> 8) | byte;
                        n += 1;
                    }
                    _ => {
                        k[3] = (k[3] >> 8) | byte;
                        doblock128x86(&mut h, &mut k);
                        n = 0;
                    }
                }
            }
        };
    }
    let i = ((16 - n) & 15) as i64;
    if i != 0 && i <= len {
        dobytes!(i);
        len -= i;
    }
    let end = ptr + (len & !15) as usize;
    while ptr < end {
        for (j, kj) in k.iter_mut().enumerate() {
            *kj = u32::from_le_bytes(key[ptr + 4 * j..ptr + 4 * j + 4].try_into().unwrap());
        }
        doblock128x86(&mut h, &mut k);
        ptr += 16;
    }
    len -= len & !15;
    dobytes!(len);
    *ph = h;
    pcarry[0] = k[0];
    pcarry[1] = k[1];
    pcarry[2] = k[2];
    pcarry[3] = (k[3] & !0xff) | n as u32;
}

fn pmurhash128x86_result(ph: &[u32; 4], pcarry: &[u32; 4], total_length: u32) -> [u32; 4] {
    let [mut h1, mut h2, mut h3, mut h4] = *ph;
    let mut k4 = pcarry[3];
    let n = k4 & 15;
    // fall-through chain of the C switch
    let mut stage = 0; // 4: k4..k1, 3: k3..k1, 2: k2..k1, 1: k1
    let mut k1 = 0u32;
    let mut k2 = 0u32;
    let mut k3 = 0u32;
    match n {
        1..=4 => {
            k1 = pcarry[0] >> ((4 - n) * 8);
            stage = 1;
        }
        5..=8 => {
            k2 = pcarry[1] >> ((8 - n) * 8);
            stage = 2;
        }
        9..=12 => {
            k3 = pcarry[2] >> ((12 - n) * 8);
            stage = 3;
        }
        13..=15 => {
            k4 >>= (16 - n) * 8;
            stage = 4;
        }
        _ => {}
    }
    if stage >= 4 {
        k4 = k4.wrapping_mul(C4);
        k4 = k4.rotate_left(18);
        k4 = k4.wrapping_mul(C1);
        h4 ^= k4;
        k3 = pcarry[2];
    }
    if stage >= 3 {
        k3 = k3.wrapping_mul(C3);
        k3 = k3.rotate_left(17);
        k3 = k3.wrapping_mul(C4);
        h3 ^= k3;
        k2 = pcarry[1];
    }
    if stage >= 2 {
        k2 = k2.wrapping_mul(C2);
        k2 = k2.rotate_left(16);
        k2 = k2.wrapping_mul(C3);
        h2 ^= k2;
        k1 = pcarry[0];
    }
    if stage >= 1 {
        k1 = k1.wrapping_mul(C1);
        k1 = k1.rotate_left(15);
        k1 = k1.wrapping_mul(C2);
        h1 ^= k1;
    }
    h1 ^= total_length;
    h2 ^= total_length;
    h3 ^= total_length;
    h4 ^= total_length;
    h1 = h1.wrapping_add(h2).wrapping_add(h3).wrapping_add(h4);
    h2 = h2.wrapping_add(h1);
    h3 = h3.wrapping_add(h1);
    h4 = h4.wrapping_add(h1);
    h1 = fmix32(h1);
    h2 = fmix32(h2);
    h3 = fmix32(h3);
    h4 = fmix32(h4);
    h1 = h1.wrapping_add(h2).wrapping_add(h3).wrapping_add(h4);
    h2 = h2.wrapping_add(h1);
    h3 = h3.wrapping_add(h1);
    h4 = h4.wrapping_add(h1);
    [h1, h2, h3, h4]
}

impl Ctx for Murmur3c {
    fn update(&mut self, data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u32);
        pmurhash128x86_process(&mut self.h, &mut self.carry, data);
    }

    fn finish(&mut self, out: &mut [u8]) {
        let h = pmurhash128x86_result(&self.h, &self.carry, self.len);
        for (i, v) in h.iter().enumerate() {
            out[4 * i..4 * i + 4].copy_from_slice(&v.to_be_bytes());
        }
    }

    fn image(&self) -> Vec<u8> {
        let mut v = vec![0u8; 36];
        let mut w = Img(&mut v);
        w.u32s(0, &self.h);
        w.u32s(16, &self.carry);
        w.u32s(32, &[self.len]);
        v
    }

    fn set_image(&mut self, img: &[u8]) {
        self.h = rd_u32s(img, 0);
        self.carry = rd_u32s(img, 16);
        self.len = rd_u32(img, 32);
    }
}

// ---------------------------------------------------------------- murmur3f

const C1L: u64 = 0x87c37b91114253d5;
const C2L: u64 = 0x4cf5ad432745937f;

#[inline(always)]
fn doblock128x64(h: &mut [u64; 2], k: &mut [u64; 2]) {
    k[0] = k[0].wrapping_mul(C1L);
    k[0] = k[0].rotate_left(31);
    k[0] = k[0].wrapping_mul(C2L);
    h[0] ^= k[0];
    h[0] = h[0].rotate_left(27);
    h[0] = h[0].wrapping_add(h[1]);
    h[0] = h[0].wrapping_mul(5).wrapping_add(0x52dce729);
    k[1] = k[1].wrapping_mul(C2L);
    k[1] = k[1].rotate_left(33);
    k[1] = k[1].wrapping_mul(C1L);
    h[1] ^= k[1];
    h[1] = h[1].rotate_left(31);
    h[1] = h[1].wrapping_add(h[0]);
    h[1] = h[1].wrapping_mul(5).wrapping_add(0x38495ab5);
}

#[derive(Clone)]
pub struct Murmur3f {
    h: [u64; 2],
    carry: [u64; 2],
    len: u32,
}

pub fn m3f_init(args: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    let s = seed_option(args).unwrap_or(0) as u64;
    Box::new(Murmur3f {
        h: [s; 2],
        carry: [0; 2],
        len: 0,
    })
}

fn pmurhash128x64_process(ph: &mut [u64; 2], pcarry: &mut [u64; 2], key: &[u8]) {
    let mut h = *ph;
    let mut k = *pcarry;
    let mut n = (k[1] & 15) as usize;
    let mut ptr = 0usize;
    let mut len = key.len() as i32 as i64;
    macro_rules! dobytes {
        ($cnt:expr) => {
            let mut cnt = $cnt;
            while cnt > 0 {
                cnt -= 1;
                let byte = (key[ptr] as u64) << 56;
                ptr += 1;
                match n {
                    0..=7 => {
                        k[0] = (k[0] >> 8) | byte;
                        n += 1;
                    }
                    8..=14 => {
                        k[1] = (k[1] >> 8) | byte;
                        n += 1;
                    }
                    _ => {
                        k[1] = (k[1] >> 8) | byte;
                        doblock128x64(&mut h, &mut k);
                        n = 0;
                    }
                }
            }
        };
    }
    let i = ((16 - n) & 15) as i64;
    if i != 0 && i <= len {
        dobytes!(i);
        len -= i;
    }
    let end = ptr + (len & !15) as usize;
    while ptr < end {
        k[0] = u64::from_le_bytes(key[ptr..ptr + 8].try_into().unwrap());
        k[1] = u64::from_le_bytes(key[ptr + 8..ptr + 16].try_into().unwrap());
        doblock128x64(&mut h, &mut k);
        ptr += 16;
    }
    len -= len & !15;
    dobytes!(len);
    *ph = h;
    pcarry[0] = k[0];
    pcarry[1] = (k[1] & !0xff) | n as u64;
}

fn pmurhash128x64_result(ph: &[u64; 2], pcarry: &[u64; 2], total_length: u32) -> [u64; 2] {
    let [mut h1, mut h2] = *ph;
    let mut k2 = pcarry[1];
    let n = k2 & 15;
    if n != 0 {
        let mut k1 = pcarry[0];
        if n > 8 {
            k2 >>= (16 - n) * 8;
            k2 = k2.wrapping_mul(C2L);
            k2 = k2.rotate_left(33);
            k2 = k2.wrapping_mul(C1L);
            h2 ^= k2;
        } else {
            k1 >>= (8 - n) * 8;
        }
        k1 = k1.wrapping_mul(C1L);
        k1 = k1.rotate_left(31);
        k1 = k1.wrapping_mul(C2L);
        h1 ^= k1;
    }
    h1 ^= total_length as u64;
    h2 ^= total_length as u64;
    h1 = h1.wrapping_add(h2);
    h2 = h2.wrapping_add(h1);
    h1 = fmix64(h1);
    h2 = fmix64(h2);
    h1 = h1.wrapping_add(h2);
    h2 = h2.wrapping_add(h1);
    [h1, h2]
}

impl Ctx for Murmur3f {
    fn update(&mut self, data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u32);
        pmurhash128x64_process(&mut self.h, &mut self.carry, data);
    }

    fn finish(&mut self, out: &mut [u8]) {
        let h = pmurhash128x64_result(&self.h, &self.carry, self.len);
        out[..8].copy_from_slice(&h[0].to_be_bytes());
        out[8..16].copy_from_slice(&h[1].to_be_bytes());
    }

    fn image(&self) -> Vec<u8> {
        let mut v = vec![0u8; 40];
        let mut w = Img(&mut v);
        w.u64s(0, &self.h);
        w.u64s(16, &self.carry);
        w.u32s(32, &[self.len]);
        v
    }

    fn set_image(&mut self, img: &[u8]) {
        self.h = rd_u64s(img, 0);
        self.carry = rd_u64s(img, 16);
        self.len = rd_u32(img, 32);
    }
}
