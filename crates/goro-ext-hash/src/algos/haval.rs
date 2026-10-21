//! HAVAL (ext/hash/hash_haval.c), 3/4/5 passes, 128..256 bit output.

use super::{encode_le, le_words, md32_update};
use crate::ops::{Ctx, Img, InitArgs, rd_bytes, rd_u32s};

static D0: [u32; 8] = [
    0x243F6A88, 0x85A308D3, 0x13198A2E, 0x03707344, 0xA4093822, 0x299F31D0, 0x082EFA98, 0xEC4E6C89,
];

static K2: [u32; 32] = [
    0x452821E6, 0x38D01377, 0xBE5466CF, 0x34E90C6C, 0xC0AC29B7, 0xC97C50DD, 0x3F84D5B5, 0xB5470917,
    0x9216D5D9, 0x8979FB1B, 0xD1310BA6, 0x98DFB5AC, 0x2FFD72DB, 0xD01ADFB7, 0xB8E1AFED, 0x6A267E96,
    0xBA7C9045, 0xF12C7F99, 0x24A19947, 0xB3916CF7, 0x0801F2E2, 0x858EFC16, 0x636920D8, 0x71574E69,
    0xA458FEA3, 0xF4933D7E, 0x0D95748F, 0x728EB658, 0x718BCD58, 0x82154AEE, 0x7B54A41D, 0xC25A59B5,
];
static K3: [u32; 32] = [
    0x9C30D539, 0x2AF26013, 0xC5D1B023, 0x286085F0, 0xCA417918, 0xB8DB38EF, 0x8E79DCB0, 0x603A180E,
    0x6C9E0E8B, 0xB01E8A3E, 0xD71577C1, 0xBD314B27, 0x78AF2FDA, 0x55605C60, 0xE65525F3, 0xAA55AB94,
    0x57489862, 0x63E81440, 0x55CA396A, 0x2AAB10B6, 0xB4CC5C34, 0x1141E8CE, 0xA15486AF, 0x7C72E993,
    0xB3EE1411, 0x636FBC2A, 0x2BA9C55D, 0x741831F6, 0xCE5C3E16, 0x9B87931E, 0xAFD6BA33, 0x6C24CF5C,
];
static K4: [u32; 32] = [
    0x7A325381, 0x28958677, 0x3B8F4898, 0x6B4BB9AF, 0xC4BFE81B, 0x66282193, 0x61D809CC, 0xFB21A991,
    0x487CAC60, 0x5DEC8032, 0xEF845D5D, 0xE98575B1, 0xDC262302, 0xEB651B88, 0x23893E81, 0xD396ACC5,
    0x0F6D6FF3, 0x83F44239, 0x2E0B4482, 0xA4842004, 0x69C8F04A, 0x9E1F9B5E, 0x21C66842, 0xF6E96C9A,
    0x670C9C61, 0xABD388F0, 0x6A51A0D2, 0xD8542F68, 0x960FA728, 0xAB5133A3, 0x6EEF0B6C, 0x137A3BE4,
];
static K5: [u32; 32] = [
    0xBA3BF050, 0x7EFB2A98, 0xA1F1651D, 0x39AF0176, 0x66CA593E, 0x82430E88, 0x8CEE8619, 0x456F9FB4,
    0x7D84A5C3, 0x3B8B5EBE, 0xE06F75D8, 0x85C12073, 0x401A449F, 0x56C16AA6, 0x4ED3AA62, 0x363F7706,
    0x1BFEDF72, 0x429B023D, 0x37D0D724, 0xD00A1248, 0xDB0FEAD3, 0x49F1C09B, 0x075372C9, 0x80991B7B,
    0x25D479D8, 0xF6E8DEF7, 0xE3FE501A, 0xB6794C3B, 0x976CE0BD, 0x04C006BA, 0xC1A94FB6, 0x409F60C4,
];
static I2: [usize; 32] = [
    5, 14, 26, 18, 11, 28, 7, 16, 0, 23, 20, 22, 1, 10, 4, 8, 30, 3, 21, 9, 17, 24, 29, 6, 19, 12,
    15, 13, 2, 25, 31, 27,
];
static I3: [usize; 32] = [
    19, 9, 4, 20, 28, 17, 8, 22, 29, 14, 25, 12, 24, 30, 16, 26, 31, 15, 7, 3, 1, 0, 18, 27, 13, 6,
    21, 10, 23, 11, 5, 2,
];
static I4: [usize; 32] = [
    24, 4, 0, 14, 2, 7, 28, 23, 26, 6, 30, 20, 18, 25, 19, 3, 22, 11, 31, 21, 8, 27, 12, 9, 1, 29,
    5, 15, 17, 10, 16, 13,
];
static I5: [usize; 32] = [
    27, 3, 21, 26, 17, 11, 20, 29, 19, 0, 12, 7, 13, 8, 31, 10, 5, 9, 14, 30, 18, 6, 28, 24, 2, 23,
    16, 22, 4, 1, 25, 15,
];

/// M_k[i] = (k - i) mod 8 (the M0..M7 tables).
#[inline(always)]
fn m(k: usize, i: usize) -> usize {
    (k + 8 - (i % 8)) % 8
}

fn f1(x6: u32, x5: u32, x4: u32, x3: u32, x2: u32, x1: u32, x0: u32) -> u32 {
    (x1 & x4) ^ (x2 & x5) ^ (x3 & x6) ^ (x0 & x1) ^ x0
}
fn f2(x6: u32, x5: u32, x4: u32, x3: u32, x2: u32, x1: u32, x0: u32) -> u32 {
    (x1 & x2 & x3)
        ^ (x2 & x4 & x5)
        ^ (x1 & x2)
        ^ (x1 & x4)
        ^ (x2 & x6)
        ^ (x3 & x5)
        ^ (x4 & x5)
        ^ (x0 & x2)
        ^ x0
}
fn f3(x6: u32, x5: u32, x4: u32, x3: u32, x2: u32, x1: u32, x0: u32) -> u32 {
    (x1 & x2 & x3) ^ (x1 & x4) ^ (x2 & x5) ^ (x3 & x6) ^ (x0 & x3) ^ x0
}
fn f4(x6: u32, x5: u32, x4: u32, x3: u32, x2: u32, x1: u32, x0: u32) -> u32 {
    (x1 & x2 & x3)
        ^ (x2 & x4 & x5)
        ^ (x3 & x4 & x6)
        ^ (x1 & x4)
        ^ (x2 & x6)
        ^ (x3 & x4)
        ^ (x3 & x5)
        ^ (x3 & x6)
        ^ (x4 & x5)
        ^ (x4 & x6)
        ^ (x0 & x4)
        ^ x0
}
fn f5(x6: u32, x5: u32, x4: u32, x3: u32, x2: u32, x1: u32, x0: u32) -> u32 {
    (x1 & x4) ^ (x2 & x5) ^ (x3 & x6) ^ (x0 & x1 & x2 & x3) ^ (x0 & x5) ^ x0
}

type Fn7 = fn(u32, u32, u32, u32, u32, u32, u32) -> u32;

/// One 32-step pass: `f` applied to E[M_k[i]] for the 7 M indices in
/// `order`, plus x[idx[i]] + k[i].
fn haval_pass(
    e: &mut [u32; 8],
    x: &[u32; 32],
    f: Fn7,
    order: [usize; 7],
    idx: Option<&[usize; 32]>,
    kc: Option<&[u32; 32]>,
) {
    for i in 0..32 {
        let v = f(
            e[m(order[0], i)],
            e[m(order[1], i)],
            e[m(order[2], i)],
            e[m(order[3], i)],
            e[m(order[4], i)],
            e[m(order[5], i)],
            e[m(order[6], i)],
        );
        let xi = match idx {
            Some(t) => x[t[i]],
            None => x[i],
        };
        let mut r = v
            .rotate_right(7)
            .wrapping_add(e[m(7, i)].rotate_right(11))
            .wrapping_add(xi);
        if let Some(k) = kc {
            r = r.wrapping_add(k[i]);
        }
        e[7 - (i % 8)] = r;
    }
}

fn transform3(state: &mut [u32; 8], block: &[u8]) {
    let x: [u32; 32] = le_words(block);
    let mut e = *state;
    haval_pass(&mut e, &x, f1, [1, 0, 3, 5, 6, 2, 4], None, None);
    haval_pass(&mut e, &x, f2, [4, 2, 1, 0, 5, 3, 6], Some(&I2), Some(&K2));
    haval_pass(&mut e, &x, f3, [6, 1, 2, 3, 4, 5, 0], Some(&I3), Some(&K3));
    for (s, v) in state.iter_mut().zip(e) {
        *s = s.wrapping_add(v);
    }
}

fn transform4(state: &mut [u32; 8], block: &[u8]) {
    let x: [u32; 32] = le_words(block);
    let mut e = *state;
    haval_pass(&mut e, &x, f1, [2, 6, 1, 4, 5, 3, 0], None, None);
    haval_pass(&mut e, &x, f2, [3, 5, 2, 0, 1, 6, 4], Some(&I2), Some(&K2));
    haval_pass(&mut e, &x, f3, [1, 4, 3, 6, 0, 2, 5], Some(&I3), Some(&K3));
    haval_pass(&mut e, &x, f4, [6, 4, 0, 5, 2, 1, 3], Some(&I4), Some(&K4));
    for (s, v) in state.iter_mut().zip(e) {
        *s = s.wrapping_add(v);
    }
}

fn transform5(state: &mut [u32; 8], block: &[u8]) {
    let x: [u32; 32] = le_words(block);
    let mut e = *state;
    haval_pass(&mut e, &x, f1, [3, 4, 1, 0, 5, 2, 6], None, None);
    haval_pass(&mut e, &x, f2, [6, 2, 1, 0, 3, 4, 5], Some(&I2), Some(&K2));
    haval_pass(&mut e, &x, f3, [2, 6, 0, 4, 3, 1, 5], Some(&I3), Some(&K3));
    haval_pass(&mut e, &x, f4, [1, 5, 3, 2, 0, 4, 6], Some(&I4), Some(&K4));
    haval_pass(&mut e, &x, f5, [2, 5, 0, 6, 4, 3, 1], Some(&I5), Some(&K5));
    for (s, v) in state.iter_mut().zip(e) {
        *s = s.wrapping_add(v);
    }
}

#[derive(Clone)]
pub struct Haval {
    state: [u32; 8],
    count: [u32; 2],
    buffer: [u8; 128],
    passes: u8,
    output: u16,
}

fn new_haval(passes: u8, output: u16) -> Box<dyn Ctx> {
    Box::new(Haval {
        state: D0,
        count: [0, 0],
        buffer: [0; 128],
        passes,
        output,
    })
}

macro_rules! haval_inits {
    ($($name:ident = ($p:expr, $b:expr)),*) => {
        $(pub fn $name(_: Option<&mut InitArgs>) -> Box<dyn Ctx> { new_haval($p, $b) })*
    };
}

haval_inits!(
    init_3_128 = (3, 128),
    init_3_160 = (3, 160),
    init_3_192 = (3, 192),
    init_3_224 = (3, 224),
    init_3_256 = (3, 256),
    init_4_128 = (4, 128),
    init_4_160 = (4, 160),
    init_4_192 = (4, 192),
    init_4_224 = (4, 224),
    init_4_256 = (4, 256),
    init_5_128 = (5, 128),
    init_5_160 = (5, 160),
    init_5_192 = (5, 192),
    init_5_224 = (5, 224),
    init_5_256 = (5, 256)
);

static HAVAL_PADDING: [u8; 128] = {
    let mut p = [0u8; 128];
    p[0] = 1;
    p
};

impl Haval {
    fn transform_fn(&self) -> fn(&mut [u32; 8], &[u8]) {
        match self.passes {
            3 => transform3,
            4 => transform4,
            _ => transform5,
        }
    }
}

impl Ctx for Haval {
    fn update(&mut self, data: &[u8]) {
        let tf = self.transform_fn();
        let st = &mut self.state;
        md32_update(&mut self.count, &mut self.buffer, data, |b| tf(st, b));
    }

    fn finish(&mut self, out: &mut [u8]) {
        let mut bits = [0u8; 10];
        bits[0] = (0x01 & 0x07) | ((self.passes & 0x07) << 3) | (((self.output & 0x03) as u8) << 6);
        bits[1] = (self.output >> 2) as u8;
        encode_le(&mut bits[2..], &self.count);
        let index = ((self.count[0] >> 3) & 0x7f) as usize;
        let pad_len = if index < 118 {
            118 - index
        } else {
            246 - index
        };
        self.update(&HAVAL_PADDING[..pad_len]);
        self.update(&bits);
        let s = &mut self.state;
        match self.output {
            128 => {
                s[3] = s[3].wrapping_add(
                    (s[7] & 0xFF000000)
                        | (s[6] & 0x00FF0000)
                        | (s[5] & 0x0000FF00)
                        | (s[4] & 0x000000FF),
                );
                s[2] = s[2].wrapping_add(
                    (((s[7] & 0x00FF0000) | (s[6] & 0x0000FF00) | (s[5] & 0x000000FF)) << 8)
                        | ((s[4] & 0xFF000000) >> 24),
                );
                s[1] = s[1].wrapping_add(
                    (((s[7] & 0x0000FF00) | (s[6] & 0x000000FF)) << 16)
                        | (((s[5] & 0xFF000000) | (s[4] & 0x00FF0000)) >> 16),
                );
                s[0] = s[0].wrapping_add(
                    ((s[7] & 0x000000FF) << 24)
                        | (((s[6] & 0xFF000000) | (s[5] & 0x00FF0000) | (s[4] & 0x0000FF00)) >> 8),
                );
                encode_le(&mut out[..16], &s[..]);
            }
            160 => {
                s[4] = s[4].wrapping_add(
                    ((s[7] & 0xFE000000) | (s[6] & 0x01F80000) | (s[5] & 0x0007F000)) >> 12,
                );
                s[3] = s[3].wrapping_add(
                    ((s[7] & 0x01F80000) | (s[6] & 0x0007F000) | (s[5] & 0x00000FC0)) >> 6,
                );
                s[2] = s[2]
                    .wrapping_add((s[7] & 0x0007F000) | (s[6] & 0x00000FC0) | (s[5] & 0x0000003F));
                s[1] = s[1].wrapping_add(
                    ((s[7] & 0x00000FC0) | (s[6] & 0x0000003F) | (s[5] & 0xFE000000))
                        .rotate_right(25),
                );
                s[0] = s[0].wrapping_add(
                    ((s[7] & 0x0000003F) | (s[6] & 0xFE000000) | (s[5] & 0x01F80000))
                        .rotate_right(19),
                );
                encode_le(&mut out[..20], &s[..]);
            }
            192 => {
                s[5] = s[5].wrapping_add(((s[7] & 0xFC000000) | (s[6] & 0x03E00000)) >> 21);
                s[4] = s[4].wrapping_add(((s[7] & 0x03E00000) | (s[6] & 0x001F0000)) >> 16);
                s[3] = s[3].wrapping_add(((s[7] & 0x001F0000) | (s[6] & 0x0000FC00)) >> 10);
                s[2] = s[2].wrapping_add(((s[7] & 0x0000FC00) | (s[6] & 0x000003E0)) >> 5);
                s[1] = s[1].wrapping_add((s[7] & 0x000003E0) | (s[6] & 0x0000001F));
                s[0] =
                    s[0].wrapping_add(((s[7] & 0x0000001F) | (s[6] & 0xFC000000)).rotate_right(26));
                encode_le(&mut out[..24], &s[..]);
            }
            224 => {
                s[6] = s[6].wrapping_add(s[7] & 0x0000000F);
                s[5] = s[5].wrapping_add((s[7] >> 4) & 0x0000001F);
                s[4] = s[4].wrapping_add((s[7] >> 9) & 0x0000000F);
                s[3] = s[3].wrapping_add((s[7] >> 13) & 0x0000001F);
                s[2] = s[2].wrapping_add((s[7] >> 18) & 0x0000000F);
                s[1] = s[1].wrapping_add((s[7] >> 22) & 0x0000001F);
                s[0] = s[0].wrapping_add((s[7] >> 27) & 0x0000001F);
                encode_le(&mut out[..28], &s[..]);
            }
            _ => encode_le(&mut out[..32], &s[..]),
        }
    }

    fn image(&self) -> Vec<u8> {
        let mut v = vec![0u8; 184];
        let mut w = Img(&mut v);
        w.u32s(0, &self.state);
        w.u32s(32, &self.count);
        w.bytes(40, &self.buffer);
        v[168] = self.passes;
        v[170..172].copy_from_slice(&self.output.to_le_bytes());
        v
    }

    fn set_image(&mut self, img: &[u8]) {
        self.state = rd_u32s(img, 0);
        self.count = rd_u32s(img, 32);
        self.buffer = rd_bytes(img, 40);
    }
}
