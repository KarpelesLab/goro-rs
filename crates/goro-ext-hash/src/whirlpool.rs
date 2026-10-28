//! Whirlpool (Barreto & Rijmen, "The WHIRLPOOL Hashing Function", 2003
//! revision; ISO/IEC 10118-3).
//!
//! The S-box is built from the E and R mini-boxes of the specification
//! and the round tables from the circulant matrix cir(1, 1, 4, 1, 8, 5, 2,
//! 9) over GF(2^8) mod x^8 + x^4 + x^3 + x^2 + 1. Input is buffered bit by
//! bit as in the designers' reference implementation (a 256-bit length,
//! a 64-byte buffer, its byte position and its bit count): PHP 8.5
//! serializes exactly those fields, so the leftover buffer bytes and the
//! bit-level position must match (observed).

use crate::engine::{Engine, Field, Kind, Val, be64, clone_box, f};

pub(crate) const WHIRLPOOL_LAYOUT: &[Field] = &[
    f(Kind::U64, 8, 0),
    f(Kind::Bytes(32), 1, 64),
    f(Kind::U32, 1, 96),
    f(Kind::U32, 1, 100),
    f(Kind::Bytes(64), 1, 104),
];

const E: [u8; 16] = [
    0x1, 0xB, 0x9, 0xC, 0xD, 0x6, 0xF, 0x3, 0xE, 0x8, 0x7, 0x4, 0xA, 0x2, 0x5, 0x0,
];
const RBOX: [u8; 16] = [
    0x7, 0xC, 0xB, 0xD, 0xE, 0x4, 0x9, 0xF, 0x6, 0x3, 0x8, 0xA, 0x2, 0x5, 0x1, 0x0,
];

const SBOX: [u8; 256] = {
    let mut einv = [0u8; 16];
    let mut i = 0;
    while i < 16 {
        einv[E[i] as usize] = i as u8;
        i += 1;
    }
    let mut s = [0u8; 256];
    let mut u = 0;
    while u < 256 {
        let a = E[u >> 4];
        let b = einv[u & 15];
        let r = RBOX[(a ^ b) as usize];
        s[u] = (E[(a ^ r) as usize] << 4) | einv[(b ^ r) as usize];
        u += 1;
    }
    s
};

const fn gmul(mut a: u8, mut b: u8) -> u8 {
    let mut p = 0u8;
    while b != 0 {
        if b & 1 != 0 {
            p ^= a;
        }
        let hi = a & 0x80;
        a <<= 1;
        if hi != 0 {
            a ^= 0x1d;
        }
        b >>= 1;
    }
    p
}

/// C[t][x]: S[x] times the circulant row, rotated right by 8t bits.
static C: [[u64; 256]; 8] = {
    const ROW: [u8; 8] = [1, 1, 4, 1, 8, 5, 2, 9];
    let mut c = [[0u64; 256]; 8];
    let mut x = 0;
    while x < 256 {
        let s = SBOX[x];
        let mut v = 0u64;
        let mut k = 0;
        while k < 8 {
            v = (v << 8) | gmul(s, ROW[k]) as u64;
            k += 1;
        }
        let mut t = 0;
        while t < 8 {
            c[t][x] = v.rotate_right(8 * t as u32);
            t += 1;
        }
        x += 1;
    }
    c
};

const ROUNDS: usize = 10;

const RC: [u64; ROUNDS + 1] = {
    let mut rc = [0u64; ROUNDS + 1];
    let mut r = 1;
    while r <= ROUNDS {
        let mut v = 0u64;
        let mut j = 0;
        while j < 8 {
            v = (v << 8) | SBOX[8 * (r - 1) + j] as u64;
            j += 1;
        }
        rc[r] = v;
        r += 1;
    }
    rc
};

#[inline(always)]
fn rho(s: &[u64; 8]) -> [u64; 8] {
    let mut l = [0u64; 8];
    for (i, out) in l.iter_mut().enumerate() {
        let mut v = 0u64;
        for t in 0..8 {
            v ^= C[t][((s[(i + 8 - t) & 7] >> (56 - 8 * t)) & 0xff) as usize];
        }
        *out = v;
    }
    l
}

fn compress(h: &mut [u64; 8], block: &[u8; 64]) {
    let mut m = [0u64; 8];
    for (i, w) in m.iter_mut().enumerate() {
        *w = be64(&block[i * 8..]);
    }
    let mut k = *h;
    let mut s = [0u64; 8];
    for i in 0..8 {
        s[i] = m[i] ^ k[i];
    }
    for &rc in &RC[1..] {
        k = rho(&k);
        k[0] ^= rc;
        let l = rho(&s);
        for i in 0..8 {
            s[i] = l[i] ^ k[i];
        }
    }
    for i in 0..8 {
        h[i] ^= s[i] ^ m[i];
    }
}

#[derive(Clone)]
pub(crate) struct Whirlpool {
    h: [u64; 8],
    bitlen: [u8; 32],
    pos: usize,
    bits: usize,
    buf: [u8; 64],
}

impl Whirlpool {
    pub(crate) fn new() -> Self {
        Whirlpool {
            h: [0; 8],
            bitlen: [0; 32],
            pos: 0,
            bits: 0,
            buf: [0; 64],
        }
    }

    fn process(&mut self) {
        let b = self.buf;
        compress(&mut self.h, &b);
    }

    fn add_length(&mut self, nbits: u64) {
        let mut carry = nbits as u128;
        for i in (0..32).rev() {
            if carry == 0 {
                break;
            }
            carry += self.bitlen[i] as u128;
            self.bitlen[i] = carry as u8;
            carry >>= 8;
        }
    }
}

impl Engine for Whirlpool {
    fn update(&mut self, data: &[u8]) {
        self.add_length((data.len() as u64).wrapping_mul(8));
        let rem = (self.bits & 7) as u32;
        // Byte-aligned fast path: whole blocks straight through.
        let mut data = data;
        if rem == 0 {
            while !data.is_empty() {
                let take = (64 - self.pos).min(data.len());
                // Byte k goes to buf[pos + k]; the reference ORs it into a
                // byte it zeroed, then zeroes the next one.
                self.buf[self.pos] |= data[0];
                self.buf[self.pos + 1..self.pos + take].copy_from_slice(&data[1..take]);
                self.pos += take;
                self.bits += 8 * take;
                data = &data[take..];
                if self.bits == 512 {
                    self.process();
                    self.bits = 0;
                    self.pos = 0;
                }
                self.buf[self.pos] = 0;
            }
            return;
        }
        // General case: the buffer holds a partial byte.
        let n = data.len();
        let mut i = 0;
        let mut b: u32;
        while n - i > 1 {
            b = data[i] as u32;
            self.buf[self.pos] |= (b >> rem) as u8;
            self.pos += 1;
            self.bits += 8 - rem as usize;
            if self.bits == 512 {
                self.process();
                self.bits = 0;
                self.pos = 0;
            }
            self.buf[self.pos] = (b << (8 - rem)) as u8;
            self.bits += rem as usize;
            i += 1;
        }
        if n == 0 {
            return;
        }
        b = data[i] as u32;
        self.buf[self.pos] |= (b >> rem) as u8;
        // rem + 8 >= 8 always holds here.
        self.pos += 1;
        self.bits += 8 - rem as usize;
        if self.bits == 512 {
            self.process();
            self.bits = 0;
            self.pos = 0;
        }
        self.buf[self.pos] = (b << (8 - rem)) as u8;
        self.bits += rem as usize;
    }

    fn finish(&mut self, out: &mut [u8]) {
        self.buf[self.pos] |= 0x80u8 >> (self.bits & 7);
        self.pos += 1;
        if self.pos > 32 {
            for b in &mut self.buf[self.pos..] {
                *b = 0;
            }
            self.process();
            self.pos = 0;
        }
        for b in &mut self.buf[self.pos..32] {
            *b = 0;
        }
        let l = self.bitlen;
        self.buf[32..].copy_from_slice(&l);
        self.process();
        for (i, w) in self.h.iter().enumerate() {
            out[i * 8..i * 8 + 8].copy_from_slice(&w.to_be_bytes());
        }
    }

    clone_box!();

    fn export(&self) -> Vec<Val> {
        let mut v: Vec<Val> = self.h.iter().map(|&w| Val::U64(w)).collect();
        v.push(Val::Bytes(self.bitlen.to_vec()));
        v.push(Val::U32(self.pos as u32));
        v.push(Val::U32(self.bits as u32));
        v.push(Val::Bytes(self.buf.to_vec()));
        v
    }

    fn import(&mut self, v: &[Val]) -> bool {
        let pos = v[9].u32() as usize;
        let bits = v[10].u32() as usize;
        if pos >= 64 || bits / 8 != pos {
            return false;
        }
        for i in 0..8 {
            self.h[i] = v[i].u64();
        }
        self.bitlen.copy_from_slice(v[8].bytes());
        self.pos = pos;
        self.bits = bits;
        self.buf.copy_from_slice(v[11].bytes());
        true
    }
}
