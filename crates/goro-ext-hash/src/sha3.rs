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

/// Keccak-f[1600], unrolled over named lanes (theta, rho, pi, chi, iota).
#[rustfmt::skip]
/// Keccak-f[1600], with BMI1's and-not when the CPU has it.
pub(crate) fn keccak_f(state: &mut [u64; 25]) {
    #[cfg(target_arch = "x86_64")]
    if std::is_x86_feature_detected!("bmi1") {
        // SAFETY: BMI1 was detected.
        unsafe { keccak_f_bmi1(state) };
        return;
    }
    keccak_f_generic(state);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "bmi1")]
#[allow(unsafe_code)]
unsafe fn keccak_f_bmi1(state: &mut [u64; 25]) {
    keccak_f_generic(state);
}

/// The permutation, unrolled over named lanes (FIPS 202 theta, rho, pi,
/// chi and iota).
#[inline(always)]
fn keccak_f_generic(state: &mut [u64; 25]) {
    let [
        mut a0,
        mut a1,
        mut a2,
        mut a3,
        mut a4,
        mut a5,
        mut a6,
        mut a7,
        mut a8,
        mut a9,
        mut a10,
        mut a11,
        mut a12,
        mut a13,
        mut a14,
        mut a15,
        mut a16,
        mut a17,
        mut a18,
        mut a19,
        mut a20,
        mut a21,
        mut a22,
        mut a23,
        mut a24,
    ] = *state;
    for &rc in RC.iter() {
        let c0 = a0 ^ a5 ^ a10 ^ a15 ^ a20;
        let c1 = a1 ^ a6 ^ a11 ^ a16 ^ a21;
        let c2 = a2 ^ a7 ^ a12 ^ a17 ^ a22;
        let c3 = a3 ^ a8 ^ a13 ^ a18 ^ a23;
        let c4 = a4 ^ a9 ^ a14 ^ a19 ^ a24;
        let d0 = c4 ^ c1.rotate_left(1);
        let d1 = c0 ^ c2.rotate_left(1);
        let d2 = c1 ^ c3.rotate_left(1);
        let d3 = c2 ^ c4.rotate_left(1);
        let d4 = c3 ^ c0.rotate_left(1);
        let b0 = (a0 ^ d0).rotate_left(RHO[0]);
        let b16 = (a5 ^ d0).rotate_left(RHO[5]);
        let b7 = (a10 ^ d0).rotate_left(RHO[10]);
        let b23 = (a15 ^ d0).rotate_left(RHO[15]);
        let b14 = (a20 ^ d0).rotate_left(RHO[20]);
        let b10 = (a1 ^ d1).rotate_left(RHO[1]);
        let b1 = (a6 ^ d1).rotate_left(RHO[6]);
        let b17 = (a11 ^ d1).rotate_left(RHO[11]);
        let b8 = (a16 ^ d1).rotate_left(RHO[16]);
        let b24 = (a21 ^ d1).rotate_left(RHO[21]);
        let b20 = (a2 ^ d2).rotate_left(RHO[2]);
        let b11 = (a7 ^ d2).rotate_left(RHO[7]);
        let b2 = (a12 ^ d2).rotate_left(RHO[12]);
        let b18 = (a17 ^ d2).rotate_left(RHO[17]);
        let b9 = (a22 ^ d2).rotate_left(RHO[22]);
        let b5 = (a3 ^ d3).rotate_left(RHO[3]);
        let b21 = (a8 ^ d3).rotate_left(RHO[8]);
        let b12 = (a13 ^ d3).rotate_left(RHO[13]);
        let b3 = (a18 ^ d3).rotate_left(RHO[18]);
        let b19 = (a23 ^ d3).rotate_left(RHO[23]);
        let b15 = (a4 ^ d4).rotate_left(RHO[4]);
        let b6 = (a9 ^ d4).rotate_left(RHO[9]);
        let b22 = (a14 ^ d4).rotate_left(RHO[14]);
        let b13 = (a19 ^ d4).rotate_left(RHO[19]);
        let b4 = (a24 ^ d4).rotate_left(RHO[24]);
        a0 = b0 ^ (!b1 & b2);
        a1 = b1 ^ (!b2 & b3);
        a2 = b2 ^ (!b3 & b4);
        a3 = b3 ^ (!b4 & b0);
        a4 = b4 ^ (!b0 & b1);
        a5 = b5 ^ (!b6 & b7);
        a6 = b6 ^ (!b7 & b8);
        a7 = b7 ^ (!b8 & b9);
        a8 = b8 ^ (!b9 & b5);
        a9 = b9 ^ (!b5 & b6);
        a10 = b10 ^ (!b11 & b12);
        a11 = b11 ^ (!b12 & b13);
        a12 = b12 ^ (!b13 & b14);
        a13 = b13 ^ (!b14 & b10);
        a14 = b14 ^ (!b10 & b11);
        a15 = b15 ^ (!b16 & b17);
        a16 = b16 ^ (!b17 & b18);
        a17 = b17 ^ (!b18 & b19);
        a18 = b18 ^ (!b19 & b15);
        a19 = b19 ^ (!b15 & b16);
        a20 = b20 ^ (!b21 & b22);
        a21 = b21 ^ (!b22 & b23);
        a22 = b22 ^ (!b23 & b24);
        a23 = b23 ^ (!b24 & b20);
        a24 = b24 ^ (!b20 & b21);
        a0 ^= rc;
    }
    *state = [
        a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16, a17, a18, a19,
        a20, a21, a22, a23, a24,
    ];
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
        // Whole blocks.
        if self.pos == 0 {
            let lanes = self.rate / 8;
            while data.len() >= self.rate {
                let (words, _) = data[..self.rate].as_chunks::<8>();
                for (lane, w) in self.a[..lanes].iter_mut().zip(words) {
                    *lane ^= u64::from_le_bytes(*w);
                }
                keccak_f(&mut self.a);
                data = &data[self.rate..];
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
