//! Checksums and small non-cryptographic hashes: Adler-32 (RFC 1950),
//! the CRC-32 variants, FNV-1/FNV-1a (32 and 64 bits) and Jenkins'
//! one-at-a-time hash.
//!
//! PHP 8.5 serializes each as its single running value (observed): the
//! Adler sums packed as s2 << 16 | s1, the CRC register before the final
//! inversion, the FNV value, and the one-at-a-time value before its final
//! avalanche.

use crate::engine::{Engine, Field, Kind, Val, clone_box, f};

pub(crate) const U32_LAYOUT: &[Field] = &[f(Kind::U32, 1, 0)];
pub(crate) const U64_LAYOUT: &[Field] = &[f(Kind::U64, 1, 0)];

// ---------------------------------------------------------------- Adler-32

#[derive(Clone)]
pub(crate) struct Adler32 {
    s: u32,
}

impl Adler32 {
    pub(crate) fn new() -> Self {
        Adler32 { s: 1 }
    }
}

impl Engine for Adler32 {
    fn update(&mut self, data: &[u8]) {
        const MOD: u32 = 65521;
        // 5552 is the largest n with 255n(n+1)/2 + (n+1)(MOD-1) < 2^32.
        let (mut a, mut b) = (self.s & 0xffff, self.s >> 16);
        for chunk in data.chunks(5552) {
            for &x in chunk {
                a += x as u32;
                b += a;
            }
            a %= MOD;
            b %= MOD;
        }
        self.s = (b << 16) | a;
    }
    fn finish(&mut self, out: &mut [u8]) {
        out.copy_from_slice(&self.s.to_be_bytes());
    }
    clone_box!();
    fn export(&self) -> Vec<Val> {
        vec![Val::U32(self.s)]
    }
    fn import(&mut self, v: &[Val]) -> bool {
        self.s = v[0].u32();
        true
    }
}

// ---------------------------------------------------------------- CRC-32

const fn reflected_tables(poly: u32) -> [[u32; 256]; 8] {
    let mut t = [[0u32; 256]; 8];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { poly ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[0][i] = c;
        i += 1;
    }
    let mut s = 1;
    while s < 8 {
        let mut i = 0;
        while i < 256 {
            let p = t[s - 1][i];
            t[s][i] = (p >> 8) ^ t[0][(p & 0xff) as usize];
            i += 1;
        }
        s += 1;
    }
    t
}

const fn normal_tables(poly: u32) -> [[u32; 256]; 8] {
    let mut t = [[0u32; 256]; 8];
    let mut i = 0;
    while i < 256 {
        let mut c = (i as u32) << 24;
        let mut k = 0;
        while k < 8 {
            c = if c & 0x8000_0000 != 0 {
                (c << 1) ^ poly
            } else {
                c << 1
            };
            k += 1;
        }
        t[0][i] = c;
        i += 1;
    }
    let mut s = 1;
    while s < 8 {
        let mut i = 0;
        while i < 256 {
            let p = t[s - 1][i];
            t[s][i] = (p << 8) ^ t[0][(p >> 24) as usize];
            i += 1;
        }
        s += 1;
    }
    t
}

/// ISO 3309 / ITU-T V.42 (zlib, PNG): `crc32b`.
static CRC32B: [[u32; 256]; 8] = reflected_tables(0xedb8_8320);
/// Castagnoli (RFC 3720): `crc32c`.
static CRC32C: [[u32; 256]; 8] = reflected_tables(0x82f6_3b78);
/// The MSB-first CRC of polynomial 0x04C11DB7 (as used by bzip2): `crc32`.
static CRC32_BZIP: [[u32; 256]; 8] = normal_tables(0x04c1_1db7);

/// Slicing-by-8 update of a reflected CRC register.
#[inline]
fn crc_reflected(t: &[[u32; 256]; 8], mut c: u32, data: &[u8]) -> u32 {
    let (chunks, chunks_rest) = data.as_chunks::<8>();
    for b in chunks {
        let lo = c ^ u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let hi = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
        c = t[7][(lo & 0xff) as usize]
            ^ t[6][((lo >> 8) & 0xff) as usize]
            ^ t[5][((lo >> 16) & 0xff) as usize]
            ^ t[4][(lo >> 24) as usize]
            ^ t[3][(hi & 0xff) as usize]
            ^ t[2][((hi >> 8) & 0xff) as usize]
            ^ t[1][((hi >> 16) & 0xff) as usize]
            ^ t[0][(hi >> 24) as usize];
    }
    for &b in chunks_rest {
        c = (c >> 8) ^ t[0][((c ^ b as u32) & 0xff) as usize];
    }
    c
}

/// Slicing-by-8 update of an MSB-first CRC register.
#[inline]
fn crc_normal(t: &[[u32; 256]; 8], mut c: u32, data: &[u8]) -> u32 {
    let (chunks, chunks_rest) = data.as_chunks::<8>();
    for b in chunks {
        let hi = c ^ u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        let lo = u32::from_be_bytes([b[4], b[5], b[6], b[7]]);
        c = t[7][(hi >> 24) as usize]
            ^ t[6][((hi >> 16) & 0xff) as usize]
            ^ t[5][((hi >> 8) & 0xff) as usize]
            ^ t[4][(hi & 0xff) as usize]
            ^ t[3][(lo >> 24) as usize]
            ^ t[2][((lo >> 16) & 0xff) as usize]
            ^ t[1][((lo >> 8) & 0xff) as usize]
            ^ t[0][(lo & 0xff) as usize];
    }
    for &b in chunks_rest {
        c = (c << 8) ^ t[0][((c >> 24) ^ b as u32) as usize];
    }
    c
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CrcKind {
    /// `crc32`: MSB-first, digest printed least significant byte first.
    Bzip,
    /// `crc32b`.
    B,
    /// `crc32c`.
    C,
}

#[derive(Clone)]
pub(crate) struct Crc32 {
    c: u32,
    kind: CrcKind,
}

impl Crc32 {
    pub(crate) fn new(kind: CrcKind) -> Self {
        Crc32 { c: !0, kind }
    }
}

impl Engine for Crc32 {
    fn update(&mut self, data: &[u8]) {
        self.c = match self.kind {
            CrcKind::Bzip => crc_normal(&CRC32_BZIP, self.c, data),
            CrcKind::B => crc_reflected(&CRC32B, self.c, data),
            CrcKind::C => crc_reflected(&CRC32C, self.c, data),
        };
    }
    fn finish(&mut self, out: &mut [u8]) {
        let v = !self.c;
        match self.kind {
            CrcKind::Bzip => out.copy_from_slice(&v.to_le_bytes()),
            _ => out.copy_from_slice(&v.to_be_bytes()),
        }
    }
    clone_box!();
    fn export(&self) -> Vec<Val> {
        vec![Val::U32(self.c)]
    }
    fn import(&mut self, v: &[Val]) -> bool {
        self.c = v[0].u32();
        true
    }
}

// ---------------------------------------------------------------- FNV

#[derive(Clone)]
pub(crate) struct Fnv32 {
    h: u32,
    a: bool,
}

impl Fnv32 {
    pub(crate) fn new(a: bool) -> Self {
        Fnv32 { h: 0x811c_9dc5, a }
    }
}

impl Engine for Fnv32 {
    fn update(&mut self, data: &[u8]) {
        const P: u32 = 0x0100_0193;
        let mut h = self.h;
        if self.a {
            for &b in data {
                h = (h ^ b as u32).wrapping_mul(P);
            }
        } else {
            for &b in data {
                h = h.wrapping_mul(P) ^ b as u32;
            }
        }
        self.h = h;
    }
    fn finish(&mut self, out: &mut [u8]) {
        out.copy_from_slice(&self.h.to_be_bytes());
    }
    clone_box!();
    fn export(&self) -> Vec<Val> {
        vec![Val::U32(self.h)]
    }
    fn import(&mut self, v: &[Val]) -> bool {
        self.h = v[0].u32();
        true
    }
}

#[derive(Clone)]
pub(crate) struct Fnv64 {
    h: u64,
    a: bool,
}

impl Fnv64 {
    pub(crate) fn new(a: bool) -> Self {
        Fnv64 {
            h: 0xcbf2_9ce4_8422_2325,
            a,
        }
    }
}

impl Engine for Fnv64 {
    fn update(&mut self, data: &[u8]) {
        const P: u64 = 0x0000_0100_0000_01b3;
        let mut h = self.h;
        if self.a {
            for &b in data {
                h = (h ^ b as u64).wrapping_mul(P);
            }
        } else {
            for &b in data {
                h = h.wrapping_mul(P) ^ b as u64;
            }
        }
        self.h = h;
    }
    fn finish(&mut self, out: &mut [u8]) {
        out.copy_from_slice(&self.h.to_be_bytes());
    }
    clone_box!();
    fn export(&self) -> Vec<Val> {
        vec![Val::U64(self.h)]
    }
    fn import(&mut self, v: &[Val]) -> bool {
        self.h = v[0].u64();
        true
    }
}

// ---------------------------------------------------------------- joaat

#[derive(Clone)]
pub(crate) struct Joaat {
    h: u32,
}

impl Joaat {
    pub(crate) fn new() -> Self {
        Joaat { h: 0 }
    }
}

impl Engine for Joaat {
    fn update(&mut self, data: &[u8]) {
        let mut h = self.h;
        for &b in data {
            h = h.wrapping_add(b as u32);
            h = h.wrapping_add(h << 10);
            h ^= h >> 6;
        }
        self.h = h;
    }
    fn finish(&mut self, out: &mut [u8]) {
        let mut h = self.h;
        h = h.wrapping_add(h << 3);
        h ^= h >> 11;
        h = h.wrapping_add(h << 15);
        out.copy_from_slice(&h.to_be_bytes());
    }
    clone_box!();
    fn export(&self) -> Vec<Val> {
        vec![Val::U32(self.h)]
    }
    fn import(&mut self, v: &[Val]) -> bool {
        self.h = v[0].u32();
        true
    }
}
