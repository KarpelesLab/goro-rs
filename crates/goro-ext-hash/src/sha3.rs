//! SHA3-224/256/384/512 (FIPS 202).
//!
//! PHP 8.5 serializes the Keccak state as 200 bytes in the
//! "lane complementing" representation (lanes 1, 2, 8, 12, 17 and 20
//! inverted, magic number 100) followed by the absorb position (observed;
//! test ext/hash/tests/hash_serialize_003.phpt lists the three forms PHP
//! builds can use). The position must stay below the rate.

use crate::engine::{Engine, Field, Kind, Val, clone_box, f, le64};

pub(crate) const SHA3_LAYOUT: &[Field] = &[f(Kind::Bytes(200), 1, 0), f(Kind::U32, 1, 204)];

/// The serialization magic number of the lane-complemented state.
pub(crate) const SHA3_MAGIC: i64 = 100;

const COMPLEMENTED: [usize; 6] = [1, 2, 8, 12, 17, 20];

/// Round constants, from the rc(t) LFSR of FIPS 202 Algorithm 5.
const RC: [u64; 24] = {
    let mut out = [0u64; 24];
    let mut r: u8 = 1; // LFSR state, x^8 + x^6 + x^5 + x^4 + 1
    let mut i = 0;
    while i < 24 {
        let mut c = 0u64;
        let mut j = 0;
        while j < 7 {
            if r & 1 != 0 {
                c |= 1u64 << ((1u32 << j) - 1);
            }
            // advance one step
            let hi = r & 0x80;
            r <<= 1;
            if hi != 0 {
                r ^= 0x71;
            }
            j += 1;
        }
        out[i] = c;
        i += 1;
    }
    out
};

/// Rotation offsets rho[x + 5y], FIPS 202 Algorithm 2.
const RHO: [u32; 25] = {
    let mut out = [0u32; 25];
    let (mut x, mut y) = (1usize, 0usize);
    let mut t = 0u32;
    while t < 24 {
        out[x + 5 * y] = ((t + 1) * (t + 2) / 2) % 64;
        let nx = y;
        let ny = (2 * x + 3 * y) % 5;
        x = nx;
        y = ny;
        t += 1;
    }
    out
};

pub(crate) fn keccak_f(a: &mut [u64; 25]) {
    for &rc in RC.iter() {
        // theta
        let mut c = [0u64; 5];
        for x in 0..5 {
            c[x] = a[x] ^ a[x + 5] ^ a[x + 10] ^ a[x + 15] ^ a[x + 20];
        }
        for x in 0..5 {
            let d = c[(x + 4) % 5] ^ c[(x + 1) % 5].rotate_left(1);
            for y in 0..5 {
                a[x + 5 * y] ^= d;
            }
        }
        // rho and pi
        let mut b = [0u64; 25];
        for x in 0..5 {
            for y in 0..5 {
                b[y + 5 * ((2 * x + 3 * y) % 5)] = a[x + 5 * y].rotate_left(RHO[x + 5 * y]);
            }
        }
        // chi
        for y in 0..5 {
            for x in 0..5 {
                a[x + 5 * y] = b[x + 5 * y] ^ (!b[(x + 1) % 5 + 5 * y] & b[(x + 2) % 5 + 5 * y]);
            }
        }
        // iota
        a[0] ^= rc;
    }
}

#[derive(Clone)]
pub(crate) struct Sha3 {
    a: [u64; 25],
    pos: usize,
    rate: usize,
    out_len: usize,
}

impl Sha3 {
    pub(crate) fn new(out_len: usize) -> Self {
        Sha3 {
            a: [0; 25],
            pos: 0,
            rate: 200 - 2 * out_len,
            out_len,
        }
    }

    #[inline]
    fn xor_byte(&mut self, i: usize, b: u8) {
        self.a[i / 8] ^= (b as u64) << (8 * (i % 8));
    }
}

impl Engine for Sha3 {
    fn update(&mut self, mut data: &[u8]) {
        // Finish a partial lane/block byte by byte.
        while !self.pos.is_multiple_of(8) && !data.is_empty() {
            self.xor_byte(self.pos, data[0]);
            data = &data[1..];
            self.pos += 1;
            if self.pos == self.rate {
                keccak_f(&mut self.a);
                self.pos = 0;
            }
        }
        // Whole lanes.
        while data.len() >= 8 {
            self.a[self.pos / 8] ^= le64(data);
            data = &data[8..];
            self.pos += 8;
            if self.pos == self.rate {
                keccak_f(&mut self.a);
                self.pos = 0;
            }
        }
        for &b in data {
            self.xor_byte(self.pos, b);
            self.pos += 1;
            if self.pos == self.rate {
                keccak_f(&mut self.a);
                self.pos = 0;
            }
        }
    }

    fn finish(&mut self, out: &mut [u8]) {
        self.xor_byte(self.pos, 0x06);
        self.xor_byte(self.rate - 1, 0x80);
        keccak_f(&mut self.a);
        for i in 0..self.out_len {
            out[i] = (self.a[i / 8] >> (8 * (i % 8))) as u8;
        }
    }

    clone_box!();

    fn export(&self) -> Vec<Val> {
        let mut s = Vec::with_capacity(200);
        for (i, lane) in self.a.iter().enumerate() {
            let l = if COMPLEMENTED.contains(&i) {
                !lane
            } else {
                *lane
            };
            s.extend_from_slice(&l.to_le_bytes());
        }
        vec![Val::Bytes(s), Val::U32(self.pos as u32)]
    }

    fn import(&mut self, v: &[Val]) -> bool {
        let pos = v[1].u32() as usize;
        if pos >= self.rate {
            return false;
        }
        let s = v[0].bytes();
        for i in 0..25 {
            let l = le64(&s[i * 8..]);
            self.a[i] = if COMPLEMENTED.contains(&i) { !l } else { l };
        }
        self.pos = pos;
        true
    }
}
