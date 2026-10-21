//! SHA-3 (ext/hash/hash_sha3.c on top of the bundled XKCP Keccak sponge,
//! generic64lc: KeccakP-1600 with lane complementing).

use crate::ops::{Ctx, InitArgs, rd_u32, rd_u64};

/// Lanes stored complemented by the lane-complementing implementation.
const COMPLEMENTED: [usize; 6] = [1, 2, 8, 12, 17, 20];

static RC: [u64; 24] = [
    0x0000000000000001,
    0x0000000000008082,
    0x800000000000808A,
    0x8000000080008000,
    0x000000000000808B,
    0x0000000080000001,
    0x8000000080008081,
    0x8000000000008009,
    0x000000000000008A,
    0x0000000000000088,
    0x0000000080008009,
    0x000000008000000A,
    0x000000008000808B,
    0x800000000000008B,
    0x8000000000008089,
    0x8000000000008003,
    0x8000000000008002,
    0x8000000000000080,
    0x000000000000800A,
    0x800000008000000A,
    0x8000000080008081,
    0x8000000000008080,
    0x0000000080000001,
    0x8000000080008008,
];

static ROT: [u32; 25] = [
    0, 1, 62, 28, 27, 36, 44, 6, 55, 20, 3, 10, 43, 25, 39, 41, 45, 15, 21, 8, 18, 2, 61, 56, 14,
];

fn keccak_f1600(a: &mut [u64; 25]) {
    for rc in RC.iter() {
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
        // rho + pi
        let mut b = [0u64; 25];
        for x in 0..5 {
            for y in 0..5 {
                b[y + 5 * ((2 * x + 3 * y) % 5)] = a[x + 5 * y].rotate_left(ROT[x + 5 * y]);
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
pub struct Sha3 {
    /// Keccak state lanes (plain representation).
    state: [u64; 25],
    /// Rate in bits.
    rate: u32,
    byte_io_index: u32,
    squeezing: u32,
    fixed_output_length: u32,
    delimited_suffix: u8,
}

fn new_sha3(bits: u32) -> Box<dyn Ctx> {
    Box::new(Sha3 {
        state: [0; 25],
        rate: 1600 - 2 * bits,
        byte_io_index: 0,
        squeezing: 0,
        fixed_output_length: bits,
        delimited_suffix: 0x06,
    })
}

pub fn sha3_224_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    new_sha3(224)
}
pub fn sha3_256_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    new_sha3(256)
}
pub fn sha3_384_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    new_sha3(384)
}
pub fn sha3_512_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    new_sha3(512)
}

impl Sha3 {
    fn add_byte(&mut self, b: u8, off: usize) {
        self.state[off / 8] ^= (b as u64) << (8 * (off % 8));
    }

    fn add_bytes(&mut self, data: &[u8], off: usize) {
        for (k, b) in data.iter().enumerate() {
            self.add_byte(*b, off + k);
        }
    }
}

impl Ctx for Sha3 {
    fn update(&mut self, data: &[u8]) {
        if self.squeezing != 0 {
            return;
        }
        let rate_bytes = (self.rate / 8) as usize;
        let mut i = 0usize;
        while i < data.len() {
            if self.byte_io_index == 0 && data.len() - i >= rate_bytes {
                while data.len() - i >= rate_bytes {
                    // full lanes
                    for l in 0..rate_bytes / 8 {
                        let o = i + 8 * l;
                        self.state[l] ^= u64::from_le_bytes(data[o..o + 8].try_into().unwrap());
                    }
                    keccak_f1600(&mut self.state);
                    i += rate_bytes;
                }
            } else {
                let room = rate_bytes - self.byte_io_index as usize;
                let partial = if data.len() - i > room {
                    room
                } else {
                    data.len() - i
                };
                let off = self.byte_io_index as usize;
                self.add_bytes(&data[i..i + partial], off);
                i += partial;
                self.byte_io_index += partial as u32;
                if self.byte_io_index as usize == rate_bytes {
                    keccak_f1600(&mut self.state);
                    self.byte_io_index = 0;
                }
            }
        }
    }

    fn finish(&mut self, out: &mut [u8]) {
        let rate_bytes = (self.rate / 8) as usize;
        let d = self.delimited_suffix;
        self.add_byte(d, self.byte_io_index as usize);
        if d >= 0x80 && self.byte_io_index as usize == rate_bytes - 1 {
            keccak_f1600(&mut self.state);
        }
        self.add_byte(0x80, rate_bytes - 1);
        keccak_f1600(&mut self.state);
        self.byte_io_index = 0;
        self.squeezing = 1;
        let n = (self.fixed_output_length / 8) as usize;
        for (k, o) in out.iter_mut().take(n).enumerate() {
            *o = (self.state[k / 8] >> (8 * (k % 8))) as u8;
        }
    }

    fn image(&self) -> Vec<u8> {
        let mut v = vec![0u8; 224];
        for (i, lane) in self.state.iter().enumerate() {
            let l = if COMPLEMENTED.contains(&i) {
                !lane
            } else {
                *lane
            };
            v[8 * i..8 * i + 8].copy_from_slice(&l.to_le_bytes());
        }
        v[200..204].copy_from_slice(&self.rate.to_le_bytes());
        v[204..208].copy_from_slice(&self.byte_io_index.to_le_bytes());
        v[208..212].copy_from_slice(&self.squeezing.to_le_bytes());
        v[212..216].copy_from_slice(&self.fixed_output_length.to_le_bytes());
        v[216] = self.delimited_suffix;
        v
    }

    fn set_image(&mut self, img: &[u8]) {
        for i in 0..25 {
            let l = rd_u64(img, 8 * i);
            self.state[i] = if COMPLEMENTED.contains(&i) { !l } else { l };
        }
        self.rate = rd_u32(img, 200);
        self.byte_io_index = rd_u32(img, 204);
        self.squeezing = rd_u32(img, 208);
        self.fixed_output_length = rd_u32(img, 212);
        self.delimited_suffix = img[216];
    }

    fn valid(&self) -> bool {
        self.byte_io_index < self.rate / 8
    }
}
