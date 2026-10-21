//! Whirlpool (ext/hash/hash_whirlpool.c).

use crate::ops::{Ctx, Img, InitArgs, rd_bytes, rd_u32, rd_u64s};
use crate::tables::{WP_C0, WP_C1, WP_C2, WP_C3, WP_C4, WP_C5, WP_C6, WP_C7, WP_RC};

const DIGESTBITS: i32 = 512;
const WBLOCKBYTES: usize = 64;
const LENGTHBYTES: usize = 32;

#[derive(Clone)]
pub struct Whirlpool {
    state: [u64; 8],
    bitlength: [u8; 32],
    pos: i32,
    bits: i32,
    data: [u8; 64],
}

pub fn init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    Box::new(Whirlpool {
        state: [0; 8],
        bitlength: [0; 32],
        pos: 0,
        bits: 0,
        data: [0; 64],
    })
}

fn round_mix(k: &[u64; 8], i: usize) -> u64 {
    WP_C0[(k[i % 8] >> 56) as usize]
        ^ WP_C1[((k[(i + 7) % 8] >> 48) & 0xff) as usize]
        ^ WP_C2[((k[(i + 6) % 8] >> 40) & 0xff) as usize]
        ^ WP_C3[((k[(i + 5) % 8] >> 32) & 0xff) as usize]
        ^ WP_C4[((k[(i + 4) % 8] >> 24) & 0xff) as usize]
        ^ WP_C5[((k[(i + 3) % 8] >> 16) & 0xff) as usize]
        ^ WP_C6[((k[(i + 2) % 8] >> 8) & 0xff) as usize]
        ^ WP_C7[(k[(i + 1) % 8] & 0xff) as usize]
}

impl Whirlpool {
    fn transform(&mut self) {
        let mut block = [0u64; 8];
        for (i, b) in block.iter_mut().enumerate() {
            *b = u64::from_be_bytes(self.data[8 * i..8 * i + 8].try_into().unwrap());
        }
        let mut k = self.state;
        let mut state = [0u64; 8];
        for i in 0..8 {
            state[i] = block[i] ^ k[i];
        }
        for rc in WP_RC.iter().skip(1) {
            let mut l = [0u64; 8];
            for (i, x) in l.iter_mut().enumerate() {
                *x = round_mix(&k, i);
            }
            l[0] ^= rc;
            k = l;
            for (i, x) in l.iter_mut().enumerate() {
                *x = round_mix(&state, i) ^ k[i];
            }
            state = l;
        }
        for i in 0..8 {
            self.state[i] ^= state[i] ^ block[i];
        }
    }
}

impl Ctx for Whirlpool {
    fn update(&mut self, source: &[u8]) {
        let mut source_bits = (source.len() as u64).wrapping_mul(8);
        let mut source_pos = 0usize;
        // input is byte aligned: sourceGap == 0
        let buffer_rem = self.bits & 7;
        let mut buffer_bits = self.bits;
        let mut buffer_pos = self.pos as usize;

        // tally the length of the added data
        let mut value = source_bits;
        let mut carry: u32 = 0;
        let mut i: i32 = 31;
        while i >= 0 && (carry != 0 || value != 0) {
            carry += self.bitlength[i as usize] as u32 + (value as u32 & 0xff);
            self.bitlength[i as usize] = carry as u8;
            carry >>= 8;
            value >>= 8;
            i -= 1;
        }

        while source_bits > 8 {
            let b = source[source_pos] as u32;
            self.data[buffer_pos] |= (b >> buffer_rem) as u8;
            buffer_pos += 1;
            buffer_bits += 8 - buffer_rem;
            if buffer_bits == DIGESTBITS {
                self.transform();
                buffer_bits = 0;
                buffer_pos = 0;
            }
            self.data[buffer_pos] = (b << (8 - buffer_rem)) as u8;
            buffer_bits += buffer_rem;
            source_bits -= 8;
            source_pos += 1;
        }
        let b: u32 = if source_bits > 0 {
            let b = source[source_pos] as u32;
            self.data[buffer_pos] |= (b >> buffer_rem) as u8;
            b
        } else {
            0
        };
        if (buffer_rem as u64) + source_bits < 8 {
            buffer_bits += source_bits as i32;
        } else {
            buffer_pos += 1;
            buffer_bits += 8 - buffer_rem;
            source_bits -= (8 - buffer_rem) as u64;
            if buffer_bits == DIGESTBITS {
                self.transform();
                buffer_bits = 0;
                buffer_pos = 0;
            }
            self.data[buffer_pos] = (b << (8 - buffer_rem)) as u8;
            buffer_bits += source_bits as i32;
        }
        self.bits = buffer_bits;
        self.pos = buffer_pos as i32;
    }

    fn finish(&mut self, out: &mut [u8]) {
        let mut buffer_pos = self.pos as usize;
        self.data[buffer_pos] |= (0x80u32 >> (self.bits & 7)) as u8;
        buffer_pos += 1;
        if buffer_pos > WBLOCKBYTES - LENGTHBYTES {
            if buffer_pos < WBLOCKBYTES {
                for b in &mut self.data[buffer_pos..] {
                    *b = 0;
                }
            }
            self.transform();
            buffer_pos = 0;
        }
        if buffer_pos < WBLOCKBYTES - LENGTHBYTES {
            for b in &mut self.data[buffer_pos..WBLOCKBYTES - LENGTHBYTES] {
                *b = 0;
            }
        }
        let bl = self.bitlength;
        self.data[WBLOCKBYTES - LENGTHBYTES..].copy_from_slice(&bl);
        self.transform();
        for (i, s) in self.state.iter().enumerate() {
            out[8 * i..8 * i + 8].copy_from_slice(&s.to_be_bytes());
        }
    }

    fn image(&self) -> Vec<u8> {
        let mut v = vec![0u8; 168];
        let mut w = Img(&mut v);
        w.u64s(0, &self.state);
        w.bytes(64, &self.bitlength);
        w.u32s(96, &[self.pos as u32, self.bits as u32]);
        w.bytes(104, &self.data);
        v
    }

    fn set_image(&mut self, img: &[u8]) {
        self.state = rd_u64s(img, 0);
        self.bitlength = rd_bytes(img, 64);
        self.pos = rd_u32(img, 96) as i32;
        self.bits = rd_u32(img, 100) as i32;
        self.data = rd_bytes(img, 104);
    }

    fn valid(&self) -> bool {
        self.pos >= 0
            && (self.pos as usize) < 64
            && self.bits >= self.pos * 8
            && self.bits < self.pos * 8 + 8
    }
}
