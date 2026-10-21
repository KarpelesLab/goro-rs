//! GOST R 34.11-94 (ext/hash/hash_gost.c), test and CryptoPro S-boxes.

use crate::ops::{Ctx, Img, InitArgs, rd_bytes, rd_u32s};
use crate::tables::{GOST_TABLES_CRYPTO, GOST_TABLES_TEST};

type Tables = &'static [u32; 1024];

#[inline(always)]
fn sbox(tables: Tables, t: u32) -> u32 {
    tables[(t & 0xff) as usize]
        ^ tables[256 + ((t >> 8) & 0xff) as usize]
        ^ tables[512 + ((t >> 16) & 0xff) as usize]
        ^ tables[768 + (t >> 24) as usize]
}

/// The R() macro: 32 rounds of GOST 28147-89 on h[i], h[i + 1].
fn encrypt(tables: Tables, key: &[u32; 8], h: &[u32; 16], i: usize) -> (u32, u32) {
    let mut r = h[i];
    let mut l = h[i + 1];
    let order: [(usize, usize); 16] = [
        (0, 1),
        (2, 3),
        (4, 5),
        (6, 7),
        (0, 1),
        (2, 3),
        (4, 5),
        (6, 7),
        (0, 1),
        (2, 3),
        (4, 5),
        (6, 7),
        (7, 6),
        (5, 4),
        (3, 2),
        (1, 0),
    ];
    for (k1, k2) in order {
        let t = key[k1].wrapping_add(r);
        l ^= sbox(tables, t);
        let t = key[k2].wrapping_add(l);
        r ^= sbox(tables, t);
    }
    // t = r; r = l; l = t;
    (r, l)
}

fn p(w: &[u32; 8]) -> [u32; 8] {
    [
        (w[0] & 0x000000ff)
            | ((w[2] & 0x000000ff) << 8)
            | ((w[4] & 0x000000ff) << 16)
            | ((w[6] & 0x000000ff) << 24),
        ((w[0] & 0x0000ff00) >> 8)
            | (w[2] & 0x0000ff00)
            | ((w[4] & 0x0000ff00) << 8)
            | ((w[6] & 0x0000ff00) << 16),
        ((w[0] & 0x00ff0000) >> 16)
            | ((w[2] & 0x00ff0000) >> 8)
            | (w[4] & 0x00ff0000)
            | ((w[6] & 0x00ff0000) << 8),
        ((w[0] & 0xff000000) >> 24)
            | ((w[2] & 0xff000000) >> 16)
            | ((w[4] & 0xff000000) >> 8)
            | (w[6] & 0xff000000),
        (w[1] & 0x000000ff)
            | ((w[3] & 0x000000ff) << 8)
            | ((w[5] & 0x000000ff) << 16)
            | ((w[7] & 0x000000ff) << 24),
        ((w[1] & 0x0000ff00) >> 8)
            | (w[3] & 0x0000ff00)
            | ((w[5] & 0x0000ff00) << 8)
            | ((w[7] & 0x0000ff00) << 16),
        ((w[1] & 0x00ff0000) >> 16)
            | ((w[3] & 0x00ff0000) >> 8)
            | (w[5] & 0x00ff0000)
            | ((w[7] & 0x00ff0000) << 8),
        ((w[1] & 0xff000000) >> 24)
            | ((w[3] & 0xff000000) >> 16)
            | ((w[5] & 0xff000000) >> 8)
            | (w[7] & 0xff000000),
    ]
}

fn a_fn(x: &mut [u32; 8]) {
    let l = x[0] ^ x[2];
    let r = x[1] ^ x[3];
    x[0] = x[2];
    x[1] = x[3];
    x[2] = x[4];
    x[3] = x[5];
    x[4] = x[6];
    x[5] = x[7];
    x[6] = l;
    x[7] = r;
}

fn aa_fn(x: &mut [u32; 8]) {
    let l = x[0];
    let r = x[2];
    x[0] = x[4];
    x[2] = x[6];
    x[4] = l ^ r;
    x[6] = x[0] ^ r;
    let l = x[1];
    let r = x[3];
    x[1] = x[5];
    x[3] = x[7];
    x[5] = l ^ r;
    x[7] = x[1] ^ r;
}

fn c_fn(x: &mut [u32; 8]) {
    x[0] ^= 0xff00ff00;
    x[1] ^= 0xff00ff00;
    x[2] ^= 0x00ff00ff;
    x[3] ^= 0x00ff00ff;
    x[4] ^= 0x00ffff00;
    x[5] ^= 0xff0000ff;
    x[6] ^= 0x000000ff;
    x[7] ^= 0xff00ffff;
}

fn gost(tables: Tables, h: &mut [u32; 16], m: &[u32; 8]) {
    let mut u: [u32; 8] = h[..8].try_into().unwrap();
    let mut v: [u32; 8] = *m;
    let mut s = [0u32; 8];
    let mut i = 0;
    while i < 8 {
        let mut w = [0u32; 8];
        for k in 0..8 {
            w[k] = u[k] ^ v[k];
        }
        let key = p(&w);
        let (l, r) = encrypt(tables, &key, h, i);
        s[i] = r;
        s[i + 1] = l;
        if i != 6 {
            a_fn(&mut u);
            if i == 2 {
                c_fn(&mut u);
            }
            aa_fn(&mut v);
        }
        i += 2;
    }
    // SHIFT12(u, m, s)
    u[0] = m[0] ^ s[6];
    u[1] = m[1] ^ s[7];
    u[2] = m[2]
        ^ (s[0] << 16)
        ^ (s[0] >> 16)
        ^ (s[0] & 0xffff)
        ^ (s[1] & 0xffff)
        ^ (s[1] >> 16)
        ^ (s[2] << 16)
        ^ s[6]
        ^ (s[6] << 16)
        ^ (s[7] & 0xffff0000)
        ^ (s[7] >> 16);
    u[3] = m[3]
        ^ (s[0] & 0xffff)
        ^ (s[0] << 16)
        ^ (s[1] & 0xffff)
        ^ (s[1] << 16)
        ^ (s[1] >> 16)
        ^ (s[2] << 16)
        ^ (s[2] >> 16)
        ^ (s[3] << 16)
        ^ s[6]
        ^ (s[6] << 16)
        ^ (s[6] >> 16)
        ^ (s[7] & 0xffff)
        ^ (s[7] << 16)
        ^ (s[7] >> 16);
    u[4] = m[4]
        ^ (s[0] & 0xffff0000)
        ^ (s[0] << 16)
        ^ (s[0] >> 16)
        ^ (s[1] & 0xffff0000)
        ^ (s[1] >> 16)
        ^ (s[2] << 16)
        ^ (s[2] >> 16)
        ^ (s[3] << 16)
        ^ (s[3] >> 16)
        ^ (s[4] << 16)
        ^ (s[6] << 16)
        ^ (s[6] >> 16)
        ^ (s[7] & 0xffff)
        ^ (s[7] << 16)
        ^ (s[7] >> 16);
    u[5] = m[5]
        ^ (s[0] << 16)
        ^ (s[0] >> 16)
        ^ (s[0] & 0xffff0000)
        ^ (s[1] & 0xffff)
        ^ s[2]
        ^ (s[2] >> 16)
        ^ (s[3] << 16)
        ^ (s[3] >> 16)
        ^ (s[4] << 16)
        ^ (s[4] >> 16)
        ^ (s[5] << 16)
        ^ (s[6] << 16)
        ^ (s[6] >> 16)
        ^ (s[7] & 0xffff0000)
        ^ (s[7] << 16)
        ^ (s[7] >> 16);
    u[6] = m[6]
        ^ s[0]
        ^ (s[1] >> 16)
        ^ (s[2] << 16)
        ^ s[3]
        ^ (s[3] >> 16)
        ^ (s[4] << 16)
        ^ (s[4] >> 16)
        ^ (s[5] << 16)
        ^ (s[5] >> 16)
        ^ s[6]
        ^ (s[6] << 16)
        ^ (s[6] >> 16)
        ^ (s[7] << 16);
    u[7] = m[7]
        ^ (s[0] & 0xffff0000)
        ^ (s[0] << 16)
        ^ (s[1] & 0xffff)
        ^ (s[1] << 16)
        ^ (s[2] >> 16)
        ^ (s[3] << 16)
        ^ s[4]
        ^ (s[4] >> 16)
        ^ (s[5] << 16)
        ^ (s[5] >> 16)
        ^ (s[6] >> 16)
        ^ (s[7] & 0xffff)
        ^ (s[7] << 16)
        ^ (s[7] >> 16);
    // SHIFT16(h, v, u)
    v[0] = h[0] ^ (u[1] << 16) ^ (u[0] >> 16);
    v[1] = h[1] ^ (u[2] << 16) ^ (u[1] >> 16);
    v[2] = h[2] ^ (u[3] << 16) ^ (u[2] >> 16);
    v[3] = h[3] ^ (u[4] << 16) ^ (u[3] >> 16);
    v[4] = h[4] ^ (u[5] << 16) ^ (u[4] >> 16);
    v[5] = h[5] ^ (u[6] << 16) ^ (u[5] >> 16);
    v[6] = h[6] ^ (u[7] << 16) ^ (u[6] >> 16);
    v[7] = h[7]
        ^ (u[0] & 0xffff0000)
        ^ (u[0] << 16)
        ^ (u[7] >> 16)
        ^ (u[1] & 0xffff0000)
        ^ (u[1] << 16)
        ^ (u[6] << 16)
        ^ (u[7] & 0xffff0000);
    // SHIFT61(h, v)
    h[0] = (v[0] & 0xffff0000)
        ^ (v[0] << 16)
        ^ (v[0] >> 16)
        ^ (v[1] >> 16)
        ^ (v[1] & 0xffff0000)
        ^ (v[2] << 16)
        ^ (v[3] >> 16)
        ^ (v[4] << 16)
        ^ (v[5] >> 16)
        ^ v[5]
        ^ (v[6] >> 16)
        ^ (v[7] << 16)
        ^ (v[7] >> 16)
        ^ (v[7] & 0xffff);
    h[1] = (v[0] << 16)
        ^ (v[0] >> 16)
        ^ (v[0] & 0xffff0000)
        ^ (v[1] & 0xffff)
        ^ v[2]
        ^ (v[2] >> 16)
        ^ (v[3] << 16)
        ^ (v[4] >> 16)
        ^ (v[5] << 16)
        ^ (v[6] << 16)
        ^ v[6]
        ^ (v[7] & 0xffff0000)
        ^ (v[7] >> 16);
    h[2] = (v[0] & 0xffff)
        ^ (v[0] << 16)
        ^ (v[1] << 16)
        ^ (v[1] >> 16)
        ^ (v[1] & 0xffff0000)
        ^ (v[2] << 16)
        ^ (v[3] >> 16)
        ^ v[3]
        ^ (v[4] << 16)
        ^ (v[5] >> 16)
        ^ v[6]
        ^ (v[6] >> 16)
        ^ (v[7] & 0xffff)
        ^ (v[7] << 16)
        ^ (v[7] >> 16);
    h[3] = (v[0] << 16)
        ^ (v[0] >> 16)
        ^ (v[0] & 0xffff0000)
        ^ (v[1] & 0xffff0000)
        ^ (v[1] >> 16)
        ^ (v[2] << 16)
        ^ (v[2] >> 16)
        ^ v[2]
        ^ (v[3] << 16)
        ^ (v[4] >> 16)
        ^ v[4]
        ^ (v[5] << 16)
        ^ (v[6] << 16)
        ^ (v[7] & 0xffff)
        ^ (v[7] >> 16);
    h[4] = (v[0] >> 16)
        ^ (v[1] << 16)
        ^ v[1]
        ^ (v[2] >> 16)
        ^ v[2]
        ^ (v[3] << 16)
        ^ (v[3] >> 16)
        ^ v[3]
        ^ (v[4] << 16)
        ^ (v[5] >> 16)
        ^ v[5]
        ^ (v[6] << 16)
        ^ (v[6] >> 16)
        ^ (v[7] << 16);
    h[5] = (v[0] << 16)
        ^ (v[0] & 0xffff0000)
        ^ (v[1] << 16)
        ^ (v[1] >> 16)
        ^ (v[1] & 0xffff0000)
        ^ (v[2] << 16)
        ^ v[2]
        ^ (v[3] >> 16)
        ^ v[3]
        ^ (v[4] << 16)
        ^ (v[4] >> 16)
        ^ v[4]
        ^ (v[5] << 16)
        ^ (v[6] << 16)
        ^ (v[6] >> 16)
        ^ v[6]
        ^ (v[7] << 16)
        ^ (v[7] >> 16)
        ^ (v[7] & 0xffff0000);
    h[6] = v[0]
        ^ v[2]
        ^ (v[2] >> 16)
        ^ v[3]
        ^ (v[3] << 16)
        ^ v[4]
        ^ (v[4] >> 16)
        ^ (v[5] << 16)
        ^ (v[5] >> 16)
        ^ v[5]
        ^ (v[6] << 16)
        ^ (v[6] >> 16)
        ^ v[6]
        ^ (v[7] << 16)
        ^ v[7];
    h[7] = v[0]
        ^ (v[0] >> 16)
        ^ (v[1] << 16)
        ^ (v[1] >> 16)
        ^ (v[2] << 16)
        ^ (v[3] >> 16)
        ^ v[3]
        ^ (v[4] << 16)
        ^ v[4]
        ^ (v[5] >> 16)
        ^ v[5]
        ^ (v[6] << 16)
        ^ (v[6] >> 16)
        ^ (v[7] << 16)
        ^ v[7];
}

#[derive(Clone)]
pub struct Gost {
    state: [u32; 16],
    count: [u32; 2],
    length: u8,
    buffer: [u8; 32],
    tables: Tables,
}

fn new_gost(tables: Tables) -> Box<dyn Ctx> {
    Box::new(Gost {
        state: [0; 16],
        count: [0; 2],
        length: 0,
        buffer: [0; 32],
        tables,
    })
}

pub fn init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    new_gost(&GOST_TABLES_TEST)
}

pub fn init_crypto(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    new_gost(&GOST_TABLES_CRYPTO)
}

impl Gost {
    fn transform(&mut self, input: &[u8]) {
        let mut data = [0u32; 8];
        let mut temp: u32 = 0;
        for i in 0..8 {
            data[i] = u32::from_le_bytes(input[4 * i..4 * i + 4].try_into().unwrap());
            self.state[i + 8] = self.state[i + 8].wrapping_add(data[i]).wrapping_add(temp);
            temp = if self.state[i + 8] < data[i] {
                1
            } else if self.state[i + 8] == data[i] {
                temp
            } else {
                0
            };
        }
        gost(self.tables, &mut self.state, &data);
    }
}

const MAX32: u64 = 0xffffffff;

impl Ctx for Gost {
    fn update(&mut self, input: &[u8]) {
        let len = input.len();
        let len8 = (len as u64).wrapping_mul(8);
        if MAX32 - (self.count[0] as u64) < len8 {
            self.count[1] = self.count[1].wrapping_add(1);
            self.count[0] = (MAX32 as u32).wrapping_sub(self.count[0]);
            self.count[0] = (len8 as u32).wrapping_sub(self.count[0]);
        } else {
            self.count[0] = self.count[0].wrapping_add(len8 as u32);
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
        let mut l = [0u32; 8];
        l[0] = self.count[0];
        l[1] = self.count[1];
        gost(self.tables, &mut self.state, &l);
        let l: [u32; 8] = self.state[8..].try_into().unwrap();
        gost(self.tables, &mut self.state, &l);
        for i in 0..8 {
            out[4 * i..4 * i + 4].copy_from_slice(&self.state[i].to_le_bytes());
        }
    }

    fn image(&self) -> Vec<u8> {
        let mut v = vec![0u8; 120];
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
