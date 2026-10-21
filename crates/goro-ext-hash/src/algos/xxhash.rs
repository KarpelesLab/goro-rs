//! xxHash (ext/hash/hash_xxhash.c): XXH32 / XXH64 streaming states ported
//! from the bundled xxhash.h, XXH3 64/128 through twox-hash.

use crate::ops::{Ctx, Img, InitArgs, rd_u32, rd_u32s, rd_u64, rd_u64s};
use goro_core::value::Value;

const P32_1: u32 = 0x9E3779B1;
const P32_2: u32 = 0x85EBCA77;
const P32_3: u32 = 0xC2B2AE3D;
const P32_4: u32 = 0x27D4EB2F;
const P32_5: u32 = 0x165667B1;

const P64_1: u64 = 0x9E3779B185EBCA87;
const P64_2: u64 = 0xC2B2AE3D27D4EB4F;
const P64_3: u64 = 0x165667B19E3779F9;
const P64_4: u64 = 0x85EBCA77C2B2AE63;
const P64_5: u64 = 0x27D4EB2F165667C5;

const SEED_DEPRECATION: &str = "Passing a seed of a type other than int is deprecated because it is the same as setting the seed to 0";

// ---------------------------------------------------------------- XXH32

#[derive(Clone)]
pub struct Xxh32 {
    total_len_32: u32,
    large_len: u32,
    v: [u32; 4],
    mem32: [u8; 16],
    memsize: u32,
    reserved: u32,
}

fn xxh32_round(acc: u32, input: u32) -> u32 {
    acc.wrapping_add(input.wrapping_mul(P32_2))
        .rotate_left(13)
        .wrapping_mul(P32_1)
}

impl Xxh32 {
    fn reset(seed: u32) -> Xxh32 {
        Xxh32 {
            total_len_32: 0,
            large_len: 0,
            v: [
                seed.wrapping_add(P32_1).wrapping_add(P32_2),
                seed.wrapping_add(P32_2),
                seed,
                seed.wrapping_sub(P32_1),
            ],
            mem32: [0; 16],
            memsize: 0,
            reserved: 0,
        }
    }

    fn round4(&mut self, p: &[u8]) {
        for (i, v) in self.v.iter_mut().enumerate() {
            let w = u32::from_le_bytes(p[4 * i..4 * i + 4].try_into().unwrap());
            *v = xxh32_round(*v, w);
        }
    }
}

pub fn xxh32_init(args: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    if let Some(a) = args {
        if let Some(seed) = a.find("seed") {
            if let Value::Long(n) = seed {
                return Box::new(Xxh32::reset(n as u32));
            }
            a.deprecated(SEED_DEPRECATION);
        }
    }
    Box::new(Xxh32::reset(0))
}

impl Ctx for Xxh32 {
    fn update(&mut self, input: &[u8]) {
        let len = input.len();
        self.total_len_32 = self.total_len_32.wrapping_add(len as u32);
        self.large_len |= ((len >= 16) | (self.total_len_32 >= 16)) as u32;
        let ms = self.memsize as usize;
        if ms + len < 16 {
            self.mem32[ms..ms + len].copy_from_slice(input);
            self.memsize += len as u32;
            return;
        }
        let mut p = 0usize;
        if ms != 0 {
            self.mem32[ms..].copy_from_slice(&input[..16 - ms]);
            let m = self.mem32;
            self.round4(&m);
            p += 16 - ms;
            self.memsize = 0;
        }
        if p + 16 <= len {
            let limit = len - 16;
            while p <= limit {
                self.round4(&input[p..p + 16]);
                p += 16;
            }
        }
        if p < len {
            self.mem32[..len - p].copy_from_slice(&input[p..]);
            self.memsize = (len - p) as u32;
        }
    }

    fn finish(&mut self, out: &mut [u8]) {
        let mut h32 = if self.large_len != 0 {
            self.v[0]
                .rotate_left(1)
                .wrapping_add(self.v[1].rotate_left(7))
                .wrapping_add(self.v[2].rotate_left(12))
                .wrapping_add(self.v[3].rotate_left(18))
        } else {
            self.v[2].wrapping_add(P32_5)
        };
        h32 = h32.wrapping_add(self.total_len_32);
        // XXH32_finalize
        let mut len = (self.memsize & 15) as usize;
        let mut p = 0usize;
        while len >= 4 {
            let w = u32::from_le_bytes(self.mem32[p..p + 4].try_into().unwrap());
            h32 = h32.wrapping_add(w.wrapping_mul(P32_3));
            h32 = h32.rotate_left(17).wrapping_mul(P32_4);
            p += 4;
            len -= 4;
        }
        while len > 0 {
            h32 = h32.wrapping_add((self.mem32[p] as u32).wrapping_mul(P32_5));
            h32 = h32.rotate_left(11).wrapping_mul(P32_1);
            p += 1;
            len -= 1;
        }
        h32 ^= h32 >> 15;
        h32 = h32.wrapping_mul(P32_2);
        h32 ^= h32 >> 13;
        h32 = h32.wrapping_mul(P32_3);
        h32 ^= h32 >> 16;
        out[..4].copy_from_slice(&h32.to_be_bytes());
    }

    fn image(&self) -> Vec<u8> {
        let mut v = vec![0u8; 48];
        let mut w = Img(&mut v);
        w.u32s(0, &[self.total_len_32, self.large_len]);
        w.u32s(8, &self.v);
        w.bytes(24, &self.mem32);
        w.u32s(40, &[self.memsize, self.reserved]);
        v
    }

    fn set_image(&mut self, img: &[u8]) {
        self.total_len_32 = rd_u32(img, 0);
        self.large_len = rd_u32(img, 4);
        self.v = rd_u32s(img, 8);
        self.mem32 = img[24..40].try_into().unwrap();
        self.memsize = rd_u32(img, 40);
        self.reserved = rd_u32(img, 44);
    }

    fn valid(&self) -> bool {
        self.memsize < 16
    }
}

// ---------------------------------------------------------------- XXH64

#[derive(Clone)]
pub struct Xxh64 {
    total_len: u64,
    v: [u64; 4],
    mem64: [u8; 32],
    memsize: u32,
    reserved32: u32,
    reserved64: u64,
}

fn xxh64_round(acc: u64, input: u64) -> u64 {
    acc.wrapping_add(input.wrapping_mul(P64_2))
        .rotate_left(31)
        .wrapping_mul(P64_1)
}

fn xxh64_merge_round(acc: u64, val: u64) -> u64 {
    let val = xxh64_round(0, val);
    (acc ^ val).wrapping_mul(P64_1).wrapping_add(P64_4)
}

impl Xxh64 {
    fn reset(seed: u64) -> Xxh64 {
        Xxh64 {
            total_len: 0,
            v: [
                seed.wrapping_add(P64_1).wrapping_add(P64_2),
                seed.wrapping_add(P64_2),
                seed,
                seed.wrapping_sub(P64_1),
            ],
            mem64: [0; 32],
            memsize: 0,
            reserved32: 0,
            reserved64: 0,
        }
    }

    fn round4(&mut self, p: &[u8]) {
        for (i, v) in self.v.iter_mut().enumerate() {
            let w = u64::from_le_bytes(p[8 * i..8 * i + 8].try_into().unwrap());
            *v = xxh64_round(*v, w);
        }
    }
}

pub fn xxh64_init(args: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    if let Some(a) = args {
        if let Some(Value::Long(n)) = a.find("seed") {
            return Box::new(Xxh64::reset(n as u64));
        }
        // (php-src also warns when no seed was given at all)
        a.deprecated(SEED_DEPRECATION);
    }
    Box::new(Xxh64::reset(0))
}

impl Ctx for Xxh64 {
    fn update(&mut self, input: &[u8]) {
        let len = input.len();
        self.total_len = self.total_len.wrapping_add(len as u64);
        let ms = self.memsize as usize;
        if ms + len < 32 {
            self.mem64[ms..ms + len].copy_from_slice(input);
            self.memsize += len as u32;
            return;
        }
        let mut p = 0usize;
        if ms != 0 {
            self.mem64[ms..].copy_from_slice(&input[..32 - ms]);
            let m = self.mem64;
            self.round4(&m);
            p += 32 - ms;
            self.memsize = 0;
        }
        if p + 32 <= len {
            let limit = len - 32;
            while p <= limit {
                self.round4(&input[p..p + 32]);
                p += 32;
            }
        }
        if p < len {
            self.mem64[..len - p].copy_from_slice(&input[p..]);
            self.memsize = (len - p) as u32;
        }
    }

    fn finish(&mut self, out: &mut [u8]) {
        let mut h64 = if self.total_len >= 32 {
            let mut h = self.v[0]
                .rotate_left(1)
                .wrapping_add(self.v[1].rotate_left(7))
                .wrapping_add(self.v[2].rotate_left(12))
                .wrapping_add(self.v[3].rotate_left(18));
            for v in self.v {
                h = xxh64_merge_round(h, v);
            }
            h
        } else {
            self.v[2].wrapping_add(P64_5)
        };
        h64 = h64.wrapping_add(self.total_len);
        // XXH64_finalize
        let mut len = (self.total_len & 31) as usize;
        let mut p = 0usize;
        let mem = &self.mem64;
        while len >= 8 {
            let k1 = xxh64_round(0, u64::from_le_bytes(mem[p..p + 8].try_into().unwrap()));
            p += 8;
            h64 ^= k1;
            h64 = h64.rotate_left(27).wrapping_mul(P64_1).wrapping_add(P64_4);
            len -= 8;
        }
        if len >= 4 {
            let w = u32::from_le_bytes(mem[p..p + 4].try_into().unwrap()) as u64;
            h64 ^= w.wrapping_mul(P64_1);
            p += 4;
            h64 = h64.rotate_left(23).wrapping_mul(P64_2).wrapping_add(P64_3);
            len -= 4;
        }
        while len > 0 {
            h64 ^= (mem[p] as u64).wrapping_mul(P64_5);
            p += 1;
            h64 = h64.rotate_left(11).wrapping_mul(P64_1);
            len -= 1;
        }
        h64 ^= h64 >> 33;
        h64 = h64.wrapping_mul(P64_2);
        h64 ^= h64 >> 29;
        h64 = h64.wrapping_mul(P64_3);
        h64 ^= h64 >> 32;
        out[..8].copy_from_slice(&h64.to_be_bytes());
    }

    fn image(&self) -> Vec<u8> {
        let mut v = vec![0u8; 88];
        let mut w = Img(&mut v);
        w.u64s(0, &[self.total_len]);
        w.u64s(8, &self.v);
        w.bytes(40, &self.mem64);
        w.u32s(72, &[self.memsize, self.reserved32]);
        w.u64s(80, &[self.reserved64]);
        v
    }

    fn set_image(&mut self, img: &[u8]) {
        self.total_len = rd_u64(img, 0);
        self.v = rd_u64s(img, 8);
        self.mem64 = img[40..72].try_into().unwrap();
        self.memsize = rd_u32(img, 72);
        self.reserved32 = rd_u32(img, 76);
        self.reserved64 = rd_u64(img, 80);
    }

    fn valid(&self) -> bool {
        self.memsize < 32
    }
}

// ---------------------------------------------------------------- XXH3

const XXH3_SECRET_SIZE_MIN: usize = 136;
const XXH3_SECRET_SIZE_MAX: usize = 256;

#[derive(Clone)]
enum Xxh3Key {
    Seed(u64),
    Secret(Vec<u8>),
}

/// XXH3 contexts can't be serialized, so the input is simply buffered and
/// hashed in one go at the end (equivalent to the streaming API).
#[derive(Clone)]
pub struct Xxh3 {
    key: Xxh3Key,
    data: Vec<u8>,
    wide: bool,
}

fn xxh3_new(args: Option<&mut InitArgs>, wide: bool) -> Box<dyn Ctx> {
    let algo = if wide { "xxh128" } else { "xxh3" };
    let mut key = Xxh3Key::Seed(0);
    if let Some(a) = args {
        let seed = a.find("seed");
        let secret = a.find("secret");
        if seed.is_some() && secret.is_some() {
            a.throw_error(&format!(
                "{}: Only one of seed or secret is to be passed for initialization",
                algo
            ));
            return Box::new(Xxh3 {
                key,
                data: Vec::new(),
                wide,
            });
        }
        if let Some(s) = &seed {
            if !matches!(s, Value::Long(_)) {
                a.deprecated(
                    "Passing a seed of a type other than int is deprecated because it is ignored",
                );
            }
        }
        if let Some(Value::Long(n)) = seed {
            key = Xxh3Key::Seed(n as u64);
        } else if let Some(sec) = secret {
            if !matches!(sec, Value::String(_)) {
                a.deprecated("Passing a secret of a type other than string is deprecated because it implicitly converts to a string, potentially hiding bugs");
            }
            let s = match a.vm.value_to_string_checked(&sec) {
                Ok(s) => s,
                Err(_) => {
                    return Box::new(Xxh3 {
                        key,
                        data: Vec::new(),
                        wide,
                    });
                }
            };
            let mut bytes = s.as_bytes().to_vec();
            if bytes.len() < XXH3_SECRET_SIZE_MIN {
                a.throw_error(&format!(
                    "{}: Secret length must be >= {} bytes, {} bytes passed",
                    algo,
                    XXH3_SECRET_SIZE_MIN,
                    bytes.len()
                ));
                return Box::new(Xxh3 {
                    key,
                    data: Vec::new(),
                    wide,
                });
            }
            if bytes.len() > XXH3_SECRET_SIZE_MAX {
                bytes.truncate(XXH3_SECRET_SIZE_MAX);
                a.warning(&format!(
                    "{}: Secret content exceeding {} bytes discarded",
                    algo, XXH3_SECRET_SIZE_MAX
                ));
            }
            key = Xxh3Key::Secret(bytes);
        }
    }
    Box::new(Xxh3 {
        key,
        data: Vec::new(),
        wide,
    })
}

pub fn xxh3_init(args: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    xxh3_new(args, false)
}

pub fn xxh128_init(args: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    xxh3_new(args, true)
}

impl Ctx for Xxh3 {
    fn update(&mut self, data: &[u8]) {
        self.data.extend_from_slice(data);
    }

    fn finish(&mut self, out: &mut [u8]) {
        use twox_hash::{XxHash3_64, XxHash3_128};
        if self.wide {
            let h = match &self.key {
                Xxh3Key::Seed(s) => XxHash3_128::oneshot_with_seed(*s, &self.data),
                Xxh3Key::Secret(sec) => {
                    XxHash3_128::oneshot_with_secret(sec, &self.data).unwrap_or_default()
                }
            };
            out[..16].copy_from_slice(&h.to_be_bytes());
        } else {
            let h = match &self.key {
                Xxh3Key::Seed(s) => XxHash3_64::oneshot_with_seed(*s, &self.data),
                Xxh3Key::Secret(sec) => {
                    XxHash3_64::oneshot_with_secret(sec, &self.data).unwrap_or_default()
                }
            };
            out[..8].copy_from_slice(&h.to_be_bytes());
        }
    }
}
