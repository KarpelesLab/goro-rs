//! GOST R 34.11-94 (RFC 5831) with the GOST 28147-89 block cipher.
//!
//! `gost` uses the test parameter set of RFC 5831 section 7.1 and
//! `gost-crypto` the CryptoPro parameter set of RFC 4357 section 11.2;
//! both start from a zero hash. Vectors are little-endian: the first
//! message byte is the least significant one.
//!
//! Serialized layout observed on PHP 8.5: the hash H and the checksum
//! SIGMA (eight words each), the 64-bit bit count (low word first), a
//! byte-wide buffer fill count (below 32) and the 32-byte buffer.

use crate::engine::{Engine, Field, Kind, Val, clone_box, f, le32};

pub(crate) const GOST_LAYOUT: &[Field] = &[
    f(Kind::U32, 16, 0),
    f(Kind::U32, 2, 64),
    f(Kind::U8, 1, 72),
    f(Kind::Bytes(32), 1, 73),
];

/// pi_1..pi_8 as rows of the RFC tables: `PI[row][j]` is pi_{j+1}(row).
type Sbox = [[u8; 8]; 16];

/// RFC 5831 section 7.1 (columns listed right to left there).
#[rustfmt::skip]
const TEST_PARAMS: Sbox = [
    [0x4, 0xE, 0x5, 0x7, 0x6, 0x4, 0xD, 0x1],
    [0xA, 0xB, 0x8, 0xD, 0xC, 0xB, 0xB, 0xF],
    [0x9, 0x4, 0x1, 0xA, 0x7, 0xA, 0x4, 0xD],
    [0x2, 0xC, 0xD, 0x1, 0x1, 0x0, 0x1, 0x0],
    [0xD, 0x6, 0xA, 0x0, 0x5, 0x7, 0x3, 0x5],
    [0x8, 0xD, 0x3, 0x8, 0xF, 0x2, 0xF, 0x7],
    [0x0, 0xF, 0x4, 0x9, 0xD, 0x1, 0x5, 0xA],
    [0xE, 0xA, 0x2, 0xF, 0x8, 0xD, 0x9, 0x4],
    [0x6, 0x2, 0xE, 0xE, 0x4, 0x3, 0x0, 0x9],
    [0xB, 0x3, 0xF, 0x4, 0xA, 0x6, 0xA, 0x2],
    [0x1, 0x8, 0xC, 0x6, 0x9, 0x8, 0xE, 0x3],
    [0xC, 0x1, 0x7, 0xC, 0xE, 0x5, 0x7, 0xE],
    [0x7, 0x0, 0x6, 0xB, 0x0, 0x9, 0x6, 0x6],
    [0xF, 0x7, 0x0, 0x2, 0x3, 0xC, 0x8, 0xB],
    [0x5, 0x5, 0x9, 0x5, 0xB, 0xF, 0x2, 0x8],
    [0x3, 0x9, 0xB, 0x3, 0x2, 0xE, 0xC, 0xC],
];

/// id-GostR3411-94-CryptoProParamSet, RFC 4357 section 11.2: 64 bytes,
/// one row per four bytes, two pi columns per byte (pi_1 in the high
/// nibble of the first byte).
const CRYPTOPRO_PACKED: [u8; 64] = [
    0xA5, 0x74, 0x77, 0xD1, 0x4F, 0xFA, 0x66, 0xE3, 0x54, 0xC7, 0x42, 0x4A, 0x60, 0xEC, 0xB4, 0x19,
    0x82, 0x90, 0x9D, 0x75, 0x1D, 0x4F, 0xC9, 0x0B, 0x3B, 0x12, 0x2F, 0x54, 0x79, 0x08, 0xA0, 0xAF,
    0xD1, 0x3E, 0x1A, 0x38, 0xC7, 0xB1, 0x81, 0xC6, 0xE6, 0x56, 0x05, 0x87, 0x03, 0x25, 0xEB, 0xFE,
    0x9C, 0x6D, 0xF8, 0x6D, 0x2E, 0xAB, 0xDE, 0x20, 0xBA, 0x89, 0x3C, 0x92, 0xF8, 0xD3, 0x53, 0xBC,
];

const fn unpack(p: &[u8; 64]) -> Sbox {
    let mut s = [[0u8; 8]; 16];
    let mut row = 0;
    while row < 16 {
        let mut j = 0;
        while j < 8 {
            let byte = p[row * 4 + j / 2];
            s[row][j] = if j % 2 == 0 { byte >> 4 } else { byte & 15 };
            j += 1;
        }
        row += 1;
    }
    s
}

/// Byte-pair lookup tables: `t[k][x]` substitutes byte k of the round
/// input (nibbles 2k and 2k+1) and applies the rotation by 11.
type Tables = [[u32; 256]; 4];

const fn tables(s: &Sbox) -> Tables {
    let mut t = [[0u32; 256]; 4];
    let mut k = 0;
    while k < 4 {
        let mut x = 0;
        while x < 256 {
            let lo = s[x & 15][2 * k] as u32;
            let hi = s[x >> 4][2 * k + 1] as u32;
            let v = (lo | (hi << 4)) << (8 * k);
            t[k][x] = v.rotate_left(11);
            x += 1;
        }
        k += 1;
    }
    t
}

static TEST_TABLES: Tables = tables(&TEST_PARAMS);
static CRYPTOPRO_TABLES: Tables = tables(&unpack(&CRYPTOPRO_PACKED));

#[inline(always)]
fn g(t: &Tables, x: u32) -> u32 {
    t[0][(x & 0xff) as usize]
        ^ t[1][((x >> 8) & 0xff) as usize]
        ^ t[2][((x >> 16) & 0xff) as usize]
        ^ t[3][(x >> 24) as usize]
}

/// GOST 28147-89 ECB encryption of the 64-bit block (n1 = low word).
fn encrypt(t: &Tables, k: &[u32; 8], n1: u32, n2: u32) -> (u32, u32) {
    let (mut a, mut b) = (n1, n2);
    for r in 0..32 {
        let ki = if r < 24 { k[r % 8] } else { k[7 - (r % 8)] };
        let tmp = b ^ g(t, a.wrapping_add(ki));
        b = a;
        a = tmp;
    }
    (b, a)
}

/// A(Y) on 64-bit words, least significant first.
#[inline]
fn a_fn(y: [u64; 4]) -> [u64; 4] {
    [y[1], y[2], y[3], y[0] ^ y[1]]
}

/// The byte permutation P.
#[inline]
fn p_fn(w: [u64; 4]) -> [u32; 8] {
    let mut b = [0u8; 32];
    for i in 0..4 {
        b[i * 8..i * 8 + 8].copy_from_slice(&w[i].to_le_bytes());
    }
    let mut o = [0u8; 32];
    for i in 0..4 {
        for k in 0..8 {
            o[i + 4 * k] = b[8 * i + k];
        }
    }
    let mut k = [0u32; 8];
    for (j, kw) in k.iter_mut().enumerate() {
        *kw = le32(&o[j * 4..]);
    }
    k
}

/// C3 of RFC 5831 section 5.1, as 64-bit words (least significant first).
const C3: [u64; 4] = [
    0xff00ff00ff00ff00,
    0x00ff00ff00ff00ff,
    0xff0000ff00ffff00,
    0xff00ffff000000ff,
];

fn to64(w: &[u32; 8]) -> [u64; 4] {
    let mut o = [0u64; 4];
    for i in 0..4 {
        o[i] = w[2 * i] as u64 | ((w[2 * i + 1] as u64) << 32);
    }
    o
}

/// PSI applied `n` times on a vector of sixteen 16-bit words.
#[inline]
fn psi(y: &mut [u16; 16], n: usize) {
    for _ in 0..n {
        let x = y[0] ^ y[1] ^ y[2] ^ y[3] ^ y[12] ^ y[15];
        y.copy_within(1.., 0);
        y[15] = x;
    }
}

fn to16(w: &[u32; 8]) -> [u16; 16] {
    let mut o = [0u16; 16];
    for i in 0..8 {
        o[2 * i] = w[i] as u16;
        o[2 * i + 1] = (w[i] >> 16) as u16;
    }
    o
}

fn from16(y: &[u16; 16]) -> [u32; 8] {
    let mut o = [0u32; 8];
    for i in 0..8 {
        o[i] = y[2 * i] as u32 | ((y[2 * i + 1] as u32) << 16);
    }
    o
}

/// The step function chi(M, H).
pub(crate) fn step(t: &Tables, h: &mut [u32; 8], m: &[u32; 8]) {
    let h64 = to64(h);
    let mut u = h64;
    let mut v = to64(m);
    let mut keys = [[0u32; 8]; 4];
    for (i, key) in keys.iter_mut().enumerate() {
        if i > 0 {
            u = a_fn(u);
            if i == 2 {
                for (uw, c) in u.iter_mut().zip(C3) {
                    *uw ^= c;
                }
            }
            v = a_fn(a_fn(v));
        }
        let w = [u[0] ^ v[0], u[1] ^ v[1], u[2] ^ v[2], u[3] ^ v[3]];
        *key = p_fn(w);
    }
    let mut s = [0u32; 8];
    for i in 0..4 {
        let (lo, hi) = encrypt(t, &keys[i], h[2 * i], h[2 * i + 1]);
        s[2 * i] = lo;
        s[2 * i + 1] = hi;
    }
    // chi = PSI^61(H xor PSI(M xor PSI^12(S)))
    let mut y = to16(&s);
    psi(&mut y, 12);
    let ms = to16(m);
    for i in 0..16 {
        y[i] ^= ms[i];
    }
    psi(&mut y, 1);
    let hs = to16(h);
    for i in 0..16 {
        y[i] ^= hs[i];
    }
    psi(&mut y, 61);
    *h = from16(&y);
}

#[derive(Clone)]
pub(crate) struct Gost {
    h: [u32; 8],
    sigma: [u32; 8],
    count: [u32; 2],
    len: usize,
    buf: [u8; 32],
    crypto: bool,
}

impl Gost {
    pub(crate) fn new(crypto: bool) -> Self {
        Gost {
            h: [0; 8],
            sigma: [0; 8],
            count: [0; 2],
            len: 0,
            buf: [0; 32],
            crypto,
        }
    }

    fn tables(&self) -> &'static Tables {
        if self.crypto {
            &CRYPTOPRO_TABLES
        } else {
            &TEST_TABLES
        }
    }

    fn block(&mut self, data: &[u8; 32]) {
        let mut m = [0u32; 8];
        for (i, w) in m.iter_mut().enumerate() {
            *w = le32(&data[i * 4..]);
        }
        let mut carry = 0u64;
        for i in 0..8 {
            let s = self.sigma[i] as u64 + m[i] as u64 + carry;
            self.sigma[i] = s as u32;
            carry = s >> 32;
        }
        let t = self.tables();
        step(t, &mut self.h, &m);
    }
}

impl Engine for Gost {
    fn update(&mut self, mut data: &[u8]) {
        let c = (((self.count[1] as u64) << 32) | self.count[0] as u64)
            .wrapping_add((data.len() as u64).wrapping_mul(8));
        self.count = [c as u32, (c >> 32) as u32];
        if self.len != 0 {
            let take = 32 - self.len;
            if data.len() < take {
                self.buf[self.len..self.len + data.len()].copy_from_slice(data);
                self.len += data.len();
                return;
            }
            self.buf[self.len..].copy_from_slice(&data[..take]);
            let b = self.buf;
            self.block(&b);
            self.buf = [0; 32];
            data = &data[take..];
            self.len = 0;
        }
        let (chunks, chunks_rest) = data.as_chunks::<32>();
        for c in chunks {
            self.block(c);
        }
        let rest = chunks_rest;
        self.buf[..rest.len()].copy_from_slice(rest);
        self.len = rest.len();
    }

    fn finish(&mut self, out: &mut [u8]) {
        if self.len != 0 {
            for b in &mut self.buf[self.len..] {
                *b = 0;
            }
            let b = self.buf;
            self.block(&b);
        }
        let t = self.tables();
        let l = [self.count[0], self.count[1], 0, 0, 0, 0, 0, 0];
        step(t, &mut self.h, &l);
        let sigma = self.sigma;
        step(t, &mut self.h, &sigma);
        for i in 0..8 {
            out[i * 4..i * 4 + 4].copy_from_slice(&self.h[i].to_le_bytes());
        }
    }

    clone_box!();

    fn export(&self) -> Vec<Val> {
        let mut v: Vec<Val> = self
            .h
            .iter()
            .chain(&self.sigma)
            .map(|&w| Val::U32(w))
            .collect();
        v.push(Val::U32(self.count[0]));
        v.push(Val::U32(self.count[1]));
        v.push(Val::U8(self.len as u8));
        v.push(Val::Bytes(self.buf.to_vec()));
        v
    }

    fn import(&mut self, v: &[Val]) -> bool {
        let len = v[18].u32() as usize;
        if len >= 32 {
            return false;
        }
        for i in 0..8 {
            self.h[i] = v[i].u32();
            self.sigma[i] = v[8 + i].u32();
        }
        self.count = [v[16].u32(), v[17].u32()];
        self.len = len;
        self.buf.copy_from_slice(v[19].bytes());
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(hex: &str) -> [u32; 8] {
        // RFC notation: most significant word first.
        let hex: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
        let mut w = [0u32; 8];
        for i in 0..8 {
            w[7 - i] = u32::from_str_radix(&hex[i * 8..i * 8 + 8], 16).unwrap();
        }
        w
    }

    /// RFC 5831 section 7.3.1: the key schedule and step function.
    #[test]
    fn rfc5831_step() {
        let m = words("73657479 62203233 3D687467 6E656C20 2C656761 7373656D 20736920 73696854");
        let k1 = p_fn(to64(&m));
        let (lo, hi) = encrypt(&TEST_TABLES, &k1, 0, 0);
        assert_eq!((hi, lo), (0x42ABBCCE, 0x32BC0B1B));
        let mut h = [0u32; 8];
        step(&TEST_TABLES, &mut h, &m);
        assert_eq!(
            h,
            words("CF9A8C65 505967A4 68A03B8C 42DE7624 D99C4124 883DA687 561C7DE3 3315C034")
        );
    }

    /// RFC 5831 section 7.3.1: the hash of the 32-byte sample message.
    #[test]
    fn rfc5831_hash() {
        let mut g = Gost::new(false);
        g.update(b"This is message, length=32 bytes");
        let mut out = [0u8; 32];
        g.finish(&mut out);
        let want = words("FAFF37A6 15A81669 1CFF3EF8 B68CA247 E09525F3 9F811983 2EB81975 D366C4B1");
        let mut wb = [0u8; 32];
        for i in 0..8 {
            wb[i * 4..i * 4 + 4].copy_from_slice(&want[i].to_le_bytes());
        }
        assert_eq!(out, wb);
    }
}
