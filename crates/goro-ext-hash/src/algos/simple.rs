//! Adler-32, CRC32 (bzip2 / ISO-HDLC / Castagnoli), FNV-1(a) and Jenkins'
//! one-at-a-time (hash_adler32.c, hash_crc32.c, hash_fnv.c, hash_joaat.c).

use crate::ops::{Ctx, InitArgs, rd_u32, rd_u64};
use crate::tables::{CRC32_TABLE, CRC32B_TABLE, CRC32C_TABLE};

/// A context made of a single 32-bit state word.
#[derive(Clone)]
pub struct Word32 {
    state: u32,
    kind: Kind32,
}

#[derive(Clone, Copy)]
enum Kind32 {
    Adler32,
    Crc32,
    Crc32b,
    Crc32c,
    Fnv132,
    Fnv1a32,
    Joaat,
}

fn w32(state: u32, kind: Kind32) -> Box<dyn Ctx> {
    Box::new(Word32 { state, kind })
}

pub fn adler32_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    w32(1, Kind32::Adler32)
}
pub fn crc32_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    w32(!0, Kind32::Crc32)
}
pub fn crc32b_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    w32(!0, Kind32::Crc32b)
}
pub fn crc32c_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    w32(!0, Kind32::Crc32c)
}
pub fn fnv132_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    w32(0x811c9dc5, Kind32::Fnv132)
}
pub fn fnv1a32_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    w32(0x811c9dc5, Kind32::Fnv1a32)
}
pub fn joaat_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    w32(0, Kind32::Joaat)
}

/// Running CRC32B (the polynomial of crc32()) over `data`, from `crc`
/// (pre-inverted state).
pub fn crc32b_update(mut crc: u32, data: &[u8]) -> u32 {
    for &b in data {
        crc = (crc >> 8) ^ CRC32B_TABLE[((crc ^ b as u32) & 0xff) as usize];
    }
    crc
}

impl Ctx for Word32 {
    fn update(&mut self, input: &[u8]) {
        match self.kind {
            Kind32::Adler32 => {
                let mut s0 = self.state & 0xffff;
                let mut s1 = (self.state >> 16) & 0xffff;
                for &b in input {
                    s0 = s0.wrapping_add(b as u32);
                    s1 = s1.wrapping_add(s0);
                    if s1 >= 0x7fffffff {
                        s0 %= 65521;
                        s1 %= 65521;
                    }
                }
                s0 %= 65521;
                s1 %= 65521;
                self.state = s0.wrapping_add(s1 << 16);
            }
            Kind32::Crc32 => {
                let mut s = self.state;
                for &b in input {
                    s = (s << 8) ^ CRC32_TABLE[((s >> 24) ^ b as u32) as usize];
                }
                self.state = s;
            }
            Kind32::Crc32b => self.state = crc32b_update(self.state, input),
            Kind32::Crc32c => {
                let mut s = self.state;
                for &b in input {
                    s = (s >> 8) ^ CRC32C_TABLE[((s ^ b as u32) & 0xff) as usize];
                }
                self.state = s;
            }
            Kind32::Fnv132 => {
                let mut h = self.state;
                for &b in input {
                    h = h.wrapping_mul(0x01000193);
                    h ^= b as u32;
                }
                self.state = h;
            }
            Kind32::Fnv1a32 => {
                let mut h = self.state;
                for &b in input {
                    h ^= b as u32;
                    h = h.wrapping_mul(0x01000193);
                }
                self.state = h;
            }
            Kind32::Joaat => {
                let mut h = self.state;
                for &b in input {
                    h = h.wrapping_add(b as u32);
                    h = h.wrapping_add(h << 10);
                    h ^= h >> 6;
                }
                self.state = h;
            }
        }
    }

    fn finish(&mut self, out: &mut [u8]) {
        let v = match self.kind {
            Kind32::Crc32 => {
                // little-endian output
                out[..4].copy_from_slice(&(!self.state).to_le_bytes());
                return;
            }
            Kind32::Crc32b | Kind32::Crc32c => !self.state,
            Kind32::Joaat => {
                let mut h = self.state;
                h = h.wrapping_add(h << 3);
                h ^= h >> 11;
                h = h.wrapping_add(h << 15);
                h
            }
            _ => self.state,
        };
        out[..4].copy_from_slice(&v.to_be_bytes());
    }

    fn image(&self) -> Vec<u8> {
        self.state.to_le_bytes().to_vec()
    }

    fn set_image(&mut self, img: &[u8]) {
        self.state = rd_u32(img, 0);
    }
}

#[derive(Clone)]
pub struct Fnv64 {
    state: u64,
    alternate: bool,
}

pub fn fnv164_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    Box::new(Fnv64 {
        state: 0xcbf29ce484222325,
        alternate: false,
    })
}

pub fn fnv1a64_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    Box::new(Fnv64 {
        state: 0xcbf29ce484222325,
        alternate: true,
    })
}

impl Ctx for Fnv64 {
    fn update(&mut self, input: &[u8]) {
        let mut h = self.state;
        for &b in input {
            if self.alternate {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            } else {
                h = h.wrapping_mul(0x100000001b3);
                h ^= b as u64;
            }
        }
        self.state = h;
    }

    fn finish(&mut self, out: &mut [u8]) {
        out[..8].copy_from_slice(&self.state.to_be_bytes());
    }

    fn image(&self) -> Vec<u8> {
        self.state.to_le_bytes().to_vec()
    }

    fn set_image(&mut self, img: &[u8]) {
        self.state = rd_u64(img, 0);
    }
}
