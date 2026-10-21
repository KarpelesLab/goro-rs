//! Tiger (ext/hash/hash_tiger.c), 3 and 4 passes.

use crate::ops::{Ctx, Img, InitArgs, rd_bytes, rd_u32, rd_u64, rd_u64s};
use crate::tables::TIGER_TABLE;

#[inline(always)]
fn t(n: usize, i: u64) -> u64 {
    TIGER_TABLE[n * 256 + (i as u8) as usize]
}

#[inline(always)]
fn round(a: &mut u64, b: &mut u64, c: &mut u64, x: u64, mul: u64) {
    *c ^= x;
    let cc = *c;
    *a = a.wrapping_sub(
        t(0, cc)
            ^ t(1, (cc as u32 >> 16) as u64)
            ^ t(2, cc >> 32)
            ^ t(3, ((cc >> 32) as u32 >> 16) as u64),
    );
    *b = b.wrapping_add(
        t(3, (cc as u32 >> 8) as u64)
            ^ t(2, (cc as u32 >> 24) as u64)
            ^ t(1, ((cc >> 32) as u32 >> 8) as u64)
            ^ t(0, ((cc >> 32) as u32 >> 24) as u64),
    );
    *b = b.wrapping_mul(mul);
}

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
    x[0] = x[0].wrapping_sub(x[7] ^ 0xA5A5A5A5A5A5A5A5);
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
    x[7] = x[7].wrapping_sub(x[6] ^ 0x0123456789ABCDEF);
}

fn compress(passes: u32, block: &[u8], state: &mut [u64; 3]) {
    let mut x = [0u64; 8];
    for (i, w) in x.iter_mut().enumerate() {
        *w = u64::from_le_bytes(block[8 * i..8 * i + 8].try_into().unwrap());
    }
    let (mut a, mut b, mut c) = (state[0], state[1], state[2]);
    let (aa, bb, cc) = (a, b, c);
    pass(&mut a, &mut b, &mut c, &x, 5);
    key_schedule(&mut x);
    pass(&mut c, &mut a, &mut b, &x, 7);
    key_schedule(&mut x);
    pass(&mut b, &mut c, &mut a, &x, 9);
    for _ in 0..passes {
        key_schedule(&mut x);
        pass(&mut a, &mut b, &mut c, &x, 9);
        let tmpa = a;
        a = c;
        c = b;
        b = tmpa;
    }
    a ^= aa;
    b = b.wrapping_sub(bb);
    c = c.wrapping_add(cc);
    *state = [a, b, c];
}

#[derive(Clone)]
pub struct Tiger {
    state: [u64; 3],
    passed: u64,
    buffer: [u8; 64],
    length: u32,
    /// 0 for 3 passes, 1 for 4 passes (`unsigned int passes:1`)
    passes: u32,
    digest: usize,
}

fn new_tiger(passes: u32, digest: usize) -> Box<dyn Ctx> {
    Box::new(Tiger {
        state: [0x0123456789ABCDEF, 0xFEDCBA9876543210, 0xF096A5B4C3B2E187],
        passed: 0,
        buffer: [0; 64],
        length: 0,
        passes,
        digest,
    })
}

pub fn init3_128(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    new_tiger(0, 16)
}
pub fn init3_160(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    new_tiger(0, 20)
}
pub fn init3_192(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    new_tiger(0, 24)
}
pub fn init4_128(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    new_tiger(1, 16)
}
pub fn init4_160(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    new_tiger(1, 20)
}
pub fn init4_192(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    new_tiger(1, 24)
}

impl Ctx for Tiger {
    fn update(&mut self, input: &[u8]) {
        let len = input.len();
        let length = self.length as usize;
        if length + len < 64 {
            self.buffer[length..length + len].copy_from_slice(input);
            self.length += len as u32;
        } else {
            let mut i = 0usize;
            let r = (length + len) % 64;
            if length != 0 {
                i = 64 - length;
                self.buffer[length..].copy_from_slice(&input[..i]);
                let b = self.buffer;
                compress(self.passes, &b, &mut self.state);
                self.buffer = [0; 64];
                self.passed = self.passed.wrapping_add(512);
            }
            while i + 64 <= len {
                self.buffer.copy_from_slice(&input[i..i + 64]);
                let b = self.buffer;
                compress(self.passes, &b, &mut self.state);
                self.passed = self.passed.wrapping_add(512);
                i += 64;
            }
            for b in &mut self.buffer[r..] {
                *b = 0;
            }
            self.buffer[..r].copy_from_slice(&input[i..i + r]);
            self.length = r as u32;
        }
    }

    fn finish(&mut self, out: &mut [u8]) {
        let mut length = self.length as usize;
        self.passed = self.passed.wrapping_add((self.length as u64) << 3);
        self.buffer[length] = 0x1;
        length += 1;
        if !length.is_multiple_of(8) {
            let n = 8 - length % 8;
            for b in &mut self.buffer[length..length + n] {
                *b = 0;
            }
            length += n;
        }
        if length > 56 {
            for b in &mut self.buffer[length..] {
                *b = 0;
            }
            let b = self.buffer;
            compress(self.passes, &b, &mut self.state);
            for b in &mut self.buffer[..56] {
                *b = 0;
            }
        } else {
            for b in &mut self.buffer[length..56] {
                *b = 0;
            }
        }
        self.buffer[56..].copy_from_slice(&self.passed.to_le_bytes());
        let b = self.buffer;
        compress(self.passes, &b, &mut self.state);
        for (i, o) in out.iter_mut().take(self.digest).enumerate() {
            *o = (self.state[i / 8] >> (8 * (i % 8))) as u8;
        }
    }

    fn image(&self) -> Vec<u8> {
        let mut v = vec![0u8; 104];
        let mut w = Img(&mut v);
        w.u64s(0, &self.state);
        w.u64s(24, &[self.passed]);
        w.bytes(32, &self.buffer);
        w.u32s(96, &[self.length, self.passes]);
        v
    }

    fn set_image(&mut self, img: &[u8]) {
        self.state = rd_u64s(img, 0);
        self.passed = rd_u64(img, 24);
        self.buffer = rd_bytes(img, 32);
        self.length = rd_u32(img, 96);
        self.passes = rd_u32(img, 100) & 1;
    }

    fn valid(&self) -> bool {
        (self.length as usize) < 64
    }
}
