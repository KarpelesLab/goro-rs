//! Tiger (Anderson & Biham, 1996): tiger128/160/192 with 3 or 4 passes.
//!
//! The compression function and S boxes follow the authors' reference
//! (biham.cs.technion.ac.il/Reports/Tiger/); padding uses the original
//! 0x01 byte. Serialized layout observed on PHP 8.5: three state words,
//! the number of bits compressed so far, the 64-byte buffer and its fill
//! count (which must stay below 64).

use crate::engine::{Engine, Field, Kind, Val, clone_box, f, le64};
use crate::tables::tiger::TIGER_SBOX;

pub(crate) const TIGER_LAYOUT: &[Field] = &[
    f(Kind::U64, 3, 0),
    f(Kind::U64, 1, 24),
    f(Kind::Bytes(64), 1, 32),
    f(Kind::U32, 1, 96),
];

#[inline(always)]
fn round(a: &mut u64, b: &mut u64, c: &mut u64, x: u64, mul: u64) {
    let t = &TIGER_SBOX;
    *c ^= x;
    let cc = *c;
    *a = a.wrapping_sub(
        t[0][(cc & 0xff) as usize]
            ^ t[1][((cc >> 16) & 0xff) as usize]
            ^ t[2][((cc >> 32) & 0xff) as usize]
            ^ t[3][((cc >> 48) & 0xff) as usize],
    );
    *b = b.wrapping_add(
        t[3][((cc >> 8) & 0xff) as usize]
            ^ t[2][((cc >> 24) & 0xff) as usize]
            ^ t[1][((cc >> 40) & 0xff) as usize]
            ^ t[0][((cc >> 56) & 0xff) as usize],
    );
    *b = b.wrapping_mul(mul);
}

#[inline(always)]
fn pass(a: &mut u64, b: &mut u64, c: &mut u64, x: &[u64; 8], mul: u64) {
    round(a, b, c, x[0], mul);
    round(b, c, a, x[1], mul);
    round(c, a, b, x[2], mul);
    round(a, b, c, x[3], mul);
    round(b, c, a, x[4], mul);
    round(c, a, b, x[5], mul);
    round(a, b, c, x[6], mul);
    round(b, c, a, x[7], mul);
}

fn key_schedule(x: &mut [u64; 8]) {
    x[0] = x[0].wrapping_sub(x[7] ^ 0xa5a5a5a5a5a5a5a5);
    x[1] ^= x[0];
    x[2] = x[2].wrapping_add(x[1]);
    x[3] = x[3].wrapping_sub(x[2] ^ ((!x[1]) << 19));
    x[4] ^= x[3];
    x[5] = x[5].wrapping_add(x[4]);
    x[6] = x[6].wrapping_sub(x[5] ^ ((!x[4]) >> 23));
    x[7] ^= x[6];
    x[0] = x[0].wrapping_add(x[7]);
    x[1] = x[1].wrapping_sub(x[0] ^ ((!x[7]) << 19));
    x[2] ^= x[1];
    x[3] = x[3].wrapping_add(x[2]);
    x[4] = x[4].wrapping_sub(x[3] ^ ((!x[2]) >> 23));
    x[5] ^= x[4];
    x[6] = x[6].wrapping_add(x[5]);
    x[7] = x[7].wrapping_sub(x[6] ^ 0x0123456789abcdef);
}

fn compress(s: &mut [u64; 3], block: &[u8; 64], passes: usize) {
    let mut x = [0u64; 8];
    for (i, w) in x.iter_mut().enumerate() {
        *w = le64(&block[i * 8..]);
    }
    let (mut a, mut b, mut c) = (s[0], s[1], s[2]);
    for p in 0..passes {
        if p != 0 {
            key_schedule(&mut x);
        }
        let mul = match p {
            0 => 5,
            1 => 7,
            _ => 9,
        };
        pass(&mut a, &mut b, &mut c, &x, mul);
        let t = a;
        a = c;
        c = b;
        b = t;
    }
    s[0] ^= a;
    s[1] = b.wrapping_sub(s[1]);
    s[2] = c.wrapping_add(s[2]);
}

#[derive(Clone)]
pub(crate) struct Tiger {
    s: [u64; 3],
    passed: u64,
    buf: [u8; 64],
    len: usize,
    passes: usize,
    out_len: usize,
}

impl Tiger {
    pub(crate) fn new(passes: usize, out_len: usize) -> Self {
        Tiger {
            s: [0x0123456789abcdef, 0xfedcba9876543210, 0xf096a5b4c3b2e187],
            passed: 0,
            buf: [0; 64],
            len: 0,
            passes,
            out_len,
        }
    }
}

impl Engine for Tiger {
    fn update(&mut self, mut data: &[u8]) {
        if self.len != 0 {
            let take = 64 - self.len;
            if data.len() < take {
                self.buf[self.len..self.len + data.len()].copy_from_slice(data);
                self.len += data.len();
                return;
            }
            self.buf[self.len..].copy_from_slice(&data[..take]);
            let b = self.buf;
            compress(&mut self.s, &b, self.passes);
            self.passed = self.passed.wrapping_add(512);
            self.buf = [0; 64];
            data = &data[take..];
            self.len = 0;
        }
        let (chunks, chunks_rest) = data.as_chunks::<64>();
        for blk in chunks {
            compress(&mut self.s, blk, self.passes);
            self.passed = self.passed.wrapping_add(512);
        }
        let rest = chunks_rest;
        self.buf[..rest.len()].copy_from_slice(rest);
        self.len = rest.len();
    }

    fn finish(&mut self, out: &mut [u8]) {
        let bits = self.passed.wrapping_add(8 * self.len as u64);
        self.buf[self.len] = 0x01;
        for b in &mut self.buf[self.len + 1..] {
            *b = 0;
        }
        if self.len >= 56 {
            let b = self.buf;
            compress(&mut self.s, &b, self.passes);
            self.buf = [0; 64];
        }
        self.buf[56..].copy_from_slice(&bits.to_le_bytes());
        let b = self.buf;
        compress(&mut self.s, &b, self.passes);
        let mut full = [0u8; 24];
        for (i, w) in self.s.iter().enumerate() {
            full[i * 8..i * 8 + 8].copy_from_slice(&w.to_le_bytes());
        }
        out.copy_from_slice(&full[..self.out_len]);
    }

    clone_box!();

    fn export(&self) -> Vec<Val> {
        vec![
            Val::U64(self.s[0]),
            Val::U64(self.s[1]),
            Val::U64(self.s[2]),
            Val::U64(self.passed),
            Val::Bytes(self.buf.to_vec()),
            Val::U32(self.len as u32),
        ]
    }

    fn import(&mut self, v: &[Val]) -> bool {
        let len = v[5].u32() as usize;
        if len >= 64 {
            return false;
        }
        self.s = [v[0].u64(), v[1].u64(), v[2].u64()];
        self.passed = v[3].u64();
        self.buf.copy_from_slice(v[4].bytes());
        self.len = len;
        true
    }
}
