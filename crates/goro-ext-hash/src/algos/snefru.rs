//! Snefru (ext/hash/hash_snefru.c).

use crate::ops::{Ctx, Img, InitArgs, rd_bytes, rd_u32s};
use crate::tables::SNEFRU_TABLES;

const SHIFTS: [u32; 4] = [16, 8, 16, 24];

fn snefru(input: &mut [u32; 16]) {
    let mut b = *input;
    for index in 0..8 {
        let t0 = &SNEFRU_TABLES[(2 * index) * 256..(2 * index + 1) * 256];
        let t1 = &SNEFRU_TABLES[(2 * index + 1) * 256..(2 * index + 2) * 256];
        for shift in SHIFTS {
            // round(L, C, N, SB): SBE = SB[C & 0xff]; L ^= SBE; N ^= SBE
            for k in 0..16 {
                let c = k;
                let l = (k + 15) % 16;
                let n = (k + 1) % 16;
                let sb = if (k / 2) % 2 == 0 { t0 } else { t1 };
                let sbe = sb[(b[c] & 0xff) as usize];
                b[l] ^= sbe;
                b[n] ^= sbe;
            }
            for x in b.iter_mut() {
                *x = x.rotate_right(shift);
            }
        }
    }
    for i in 0..8 {
        input[i] ^= b[15 - i];
    }
}

#[derive(Clone)]
pub struct Snefru {
    state: [u32; 16],
    count: [u32; 2],
    length: u8,
    buffer: [u8; 32],
}

pub fn init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    Box::new(Snefru {
        state: [0; 16],
        count: [0; 2],
        length: 0,
        buffer: [0; 32],
    })
}

impl Snefru {
    fn transform(&mut self, input: &[u8]) {
        for j in 0..8 {
            self.state[8 + j] = u32::from_be_bytes(input[4 * j..4 * j + 4].try_into().unwrap());
        }
        snefru(&mut self.state);
        for x in &mut self.state[8..] {
            *x = 0;
        }
    }
}

const MAX32: u64 = 0xffffffff;

impl Ctx for Snefru {
    fn update(&mut self, input: &[u8]) {
        let len = input.len();
        let len8 = (len as u64).wrapping_mul(8);
        if MAX32 - (self.count[1] as u64) < len8 {
            self.count[0] = self.count[0].wrapping_add(1);
            self.count[1] = (MAX32 as u32).wrapping_sub(self.count[1]);
            self.count[1] = (len8 as u32).wrapping_sub(self.count[1]);
        } else {
            self.count[1] = self.count[1].wrapping_add(len8 as u32);
        }
        let length = self.length as usize;
        if length + len < 32 {
            self.buffer[length..length + len].copy_from_slice(input);
            self.length = self.length.wrapping_add(len as u8);
        } else {
            let mut i = 0usize;
            let r = (length + len) % 32;
            if length != 0 {
                i = 32 - length;
                self.buffer[length..].copy_from_slice(&input[..i]);
                let b = self.buffer;
                self.transform(&b);
            }
            while i + 32 <= len {
                self.transform(&input[i..i + 32]);
                i += 32;
            }
            self.buffer[..r].copy_from_slice(&input[i..i + r]);
            for b in &mut self.buffer[r..] {
                *b = 0;
            }
            self.length = r as u8;
        }
    }

    fn finish(&mut self, out: &mut [u8]) {
        if self.length != 0 {
            let b = self.buffer;
            self.transform(&b);
        }
        self.state[14] = self.count[0];
        self.state[15] = self.count[1];
        snefru(&mut self.state);
        for i in 0..8 {
            out[4 * i..4 * i + 4].copy_from_slice(&self.state[i].to_be_bytes());
        }
    }

    fn image(&self) -> Vec<u8> {
        let mut v = vec![0u8; 108];
        let mut w = Img(&mut v);
        w.u32s(0, &self.state);
        w.u32s(64, &self.count);
        v[72] = self.length;
        Img(&mut v).bytes(73, &self.buffer);
        v
    }

    fn set_image(&mut self, img: &[u8]) {
        self.state = rd_u32s(img, 0);
        self.count = rd_u32s(img, 64);
        self.length = img[72];
        self.buffer = rd_bytes(img, 73);
    }

    fn valid(&self) -> bool {
        (self.length as usize) < 32
    }
}
