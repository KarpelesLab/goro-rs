//! MD2, MD4 (ext/hash/hash_md.c) and MD5 (ext/standard/md5.c).

use super::{PADDING, encode_le, le_words, md32_update};
use crate::ops::{Ctx, Img, InitArgs, rd_bytes, rd_u32, rd_u32s};

// ---------------------------------------------------------------- MD2

static MD2_S: [u8; 256] = [
    41, 46, 67, 201, 162, 216, 124, 1, 61, 54, 84, 161, 236, 240, 6, 19, 98, 167, 5, 243, 192, 199,
    115, 140, 152, 147, 43, 217, 188, 76, 130, 202, 30, 155, 87, 60, 253, 212, 224, 22, 103, 66,
    111, 24, 138, 23, 229, 18, 190, 78, 196, 214, 218, 158, 222, 73, 160, 251, 245, 142, 187, 47,
    238, 122, 169, 104, 121, 145, 21, 178, 7, 63, 148, 194, 16, 137, 11, 34, 95, 33, 128, 127, 93,
    154, 90, 144, 50, 39, 53, 62, 204, 231, 191, 247, 151, 3, 255, 25, 48, 179, 72, 165, 181, 209,
    215, 94, 146, 42, 172, 86, 170, 198, 79, 184, 56, 210, 150, 164, 125, 182, 118, 252, 107, 226,
    156, 116, 4, 241, 69, 157, 112, 89, 100, 113, 135, 32, 134, 91, 207, 101, 230, 45, 168, 2, 27,
    96, 37, 173, 174, 176, 185, 246, 28, 70, 97, 105, 52, 64, 126, 15, 85, 71, 163, 35, 221, 81,
    175, 58, 195, 92, 249, 206, 186, 197, 234, 38, 44, 83, 13, 110, 133, 40, 132, 9, 211, 223, 205,
    244, 65, 129, 77, 82, 106, 220, 55, 200, 108, 193, 171, 250, 36, 225, 123, 8, 12, 189, 177, 74,
    120, 136, 149, 139, 227, 99, 232, 109, 233, 203, 213, 254, 59, 0, 29, 57, 242, 239, 183, 14,
    102, 88, 208, 228, 166, 119, 114, 248, 235, 117, 75, 10, 49, 68, 80, 180, 143, 237, 31, 26,
    219, 153, 141, 51, 159, 17, 131, 20,
];

#[derive(Clone)]
pub struct Md2 {
    state: [u8; 48],
    checksum: [u8; 16],
    buffer: [u8; 16],
    in_buffer: u8,
}

pub fn md2_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    Box::new(Md2 {
        state: [0; 48],
        checksum: [0; 16],
        buffer: [0; 16],
        in_buffer: 0,
    })
}

impl Md2 {
    #[allow(clippy::needless_range_loop)] // mirrors MD2_Transform
    fn transform(&mut self, block: &[u8]) {
        for i in 0..16 {
            self.state[16 + i] = block[i];
            self.state[32 + i] = self.state[16 + i] ^ self.state[i];
        }
        let mut t: u8 = 0;
        for i in 0..18u8 {
            for j in 0..48 {
                self.state[j] ^= MD2_S[t as usize];
                t = self.state[j];
            }
            t = t.wrapping_add(i);
        }
        let mut t = self.checksum[15];
        for i in 0..16 {
            self.checksum[i] ^= MD2_S[(block[i] ^ t) as usize];
            t = self.checksum[i];
        }
    }
}

impl Ctx for Md2 {
    fn update(&mut self, buf: &[u8]) {
        let mut p = 0usize;
        let len = buf.len();
        if self.in_buffer != 0 {
            let ib = self.in_buffer as usize;
            if ib + len < 16 {
                self.buffer[ib..ib + len].copy_from_slice(buf);
                self.in_buffer = self.in_buffer.wrapping_add(len as u8);
                return;
            }
            self.buffer[ib..].copy_from_slice(&buf[..16 - ib]);
            let b = self.buffer;
            self.transform(&b);
            p += 16 - ib;
            self.in_buffer = 0;
        }
        while p + 16 <= len {
            self.transform(&buf[p..p + 16]);
            p += 16;
        }
        if p < len {
            self.buffer[..len - p].copy_from_slice(&buf[p..]);
            self.in_buffer = (len - p) as u8;
        }
    }

    fn finish(&mut self, out: &mut [u8]) {
        let ib = self.in_buffer as usize;
        let pad = (16 - ib) as u8;
        for b in &mut self.buffer[ib..] {
            *b = pad;
        }
        let b = self.buffer;
        self.transform(&b);
        let c = self.checksum;
        self.transform(&c);
        out[..16].copy_from_slice(&self.state[..16]);
    }

    fn image(&self) -> Vec<u8> {
        let mut v = vec![0u8; 81];
        let mut w = Img(&mut v);
        w.bytes(0, &self.state);
        w.bytes(48, &self.checksum);
        w.bytes(64, &self.buffer);
        v[80] = self.in_buffer;
        v
    }

    fn set_image(&mut self, img: &[u8]) {
        self.state = rd_bytes(img, 0);
        self.checksum = rd_bytes(img, 48);
        self.buffer = rd_bytes(img, 64);
        self.in_buffer = img[80];
    }

    fn valid(&self) -> bool {
        (self.in_buffer as usize) < 16
    }
}

// ---------------------------------------------------------------- MD4

#[derive(Clone)]
pub struct Md4 {
    state: [u32; 4],
    count: [u32; 2],
    buffer: [u8; 64],
}

pub fn md4_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    Box::new(Md4 {
        state: [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476],
        count: [0, 0],
        buffer: [0; 64],
    })
}

fn md4_transform(state: &mut [u32; 4], block: &[u8]) {
    let x: [u32; 16] = le_words(block);
    let (mut a, mut b, mut c, mut d) = (state[0], state[1], state[2], state[3]);
    let f = |x: u32, y: u32, z: u32| z ^ (x & (y ^ z));
    let g = |x: u32, y: u32, z: u32| (x & (y | z)) | (y & z);
    let h = |x: u32, y: u32, z: u32| x ^ y ^ z;
    macro_rules! r1 {
        ($a:ident,$b:ident,$c:ident,$d:ident,$k:expr,$s:expr) => {
            $a = $a
                .wrapping_add(f($b, $c, $d))
                .wrapping_add(x[$k])
                .rotate_left($s)
        };
    }
    macro_rules! r2 {
        ($a:ident,$b:ident,$c:ident,$d:ident,$k:expr,$s:expr) => {
            $a = $a
                .wrapping_add(g($b, $c, $d))
                .wrapping_add(x[$k])
                .wrapping_add(0x5A827999)
                .rotate_left($s)
        };
    }
    macro_rules! r3 {
        ($a:ident,$b:ident,$c:ident,$d:ident,$k:expr,$s:expr) => {
            $a = $a
                .wrapping_add(h($b, $c, $d))
                .wrapping_add(x[$k])
                .wrapping_add(0x6ED9EBA1)
                .rotate_left($s)
        };
    }
    for k in [0, 4, 8, 12] {
        r1!(a, b, c, d, k, 3);
        r1!(d, a, b, c, k + 1, 7);
        r1!(c, d, a, b, k + 2, 11);
        r1!(b, c, d, a, k + 3, 19);
    }
    for k in [0, 1, 2, 3] {
        r2!(a, b, c, d, k, 3);
        r2!(d, a, b, c, k + 4, 5);
        r2!(c, d, a, b, k + 8, 9);
        r2!(b, c, d, a, k + 12, 13);
    }
    for k in [0, 2, 1, 3] {
        r3!(a, b, c, d, k, 3);
        r3!(d, a, b, c, k + 8, 9);
        r3!(c, d, a, b, k + 4, 11);
        r3!(b, c, d, a, k + 12, 15);
    }
    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
}

/// Final of the little-endian MD family (MD4, RIPEMD).
pub fn md32_le_final<const S: usize>(
    state: &mut [u32; S],
    count: &mut [u32; 2],
    buffer: &mut [u8; 64],
    out: &mut [u8],
    tf: fn(&mut [u32; S], &[u8]),
) {
    let mut bits = [0u8; 8];
    encode_le(&mut bits, &count[..]);
    let index = ((count[0] >> 3) & 0x3f) as usize;
    let pad_len = if index < 56 { 56 - index } else { 120 - index };
    md32_update(count, buffer, &PADDING[..pad_len], |b| tf(state, b));
    md32_update(count, buffer, &bits, |b| tf(state, b));
    encode_le(out, &state[..]);
}

impl Ctx for Md4 {
    fn update(&mut self, data: &[u8]) {
        let st = &mut self.state;
        md32_update(&mut self.count, &mut self.buffer, data, |b| {
            md4_transform(st, b)
        });
    }

    fn finish(&mut self, out: &mut [u8]) {
        md32_le_final(
            &mut self.state,
            &mut self.count,
            &mut self.buffer,
            &mut out[..16],
            md4_transform,
        );
    }

    fn image(&self) -> Vec<u8> {
        let mut v = vec![0u8; 88];
        let mut w = Img(&mut v);
        w.u32s(0, &self.state);
        w.u32s(16, &self.count);
        w.bytes(24, &self.buffer);
        v
    }

    fn set_image(&mut self, img: &[u8]) {
        self.state = rd_u32s(img, 0);
        self.count = rd_u32s(img, 16);
        self.buffer = rd_bytes(img, 24);
    }
}

// ---------------------------------------------------------------- MD5

#[derive(Clone)]
pub struct Md5 {
    lo: u32,
    hi: u32,
    s: [u32; 4],
    buffer: [u8; 64],
    /// Never written on x86 (the C code reads the input words directly),
    /// but part of the serialized state.
    block: [u32; 16],
}

pub fn md5_init(_: Option<&mut InitArgs>) -> Box<dyn Ctx> {
    Box::new(Md5::new())
}

impl Md5 {
    pub fn new() -> Md5 {
        Md5 {
            lo: 0,
            hi: 0,
            s: [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476],
            buffer: [0; 64],
            block: [0; 16],
        }
    }
}

impl Default for Md5 {
    fn default() -> Self {
        Self::new()
    }
}

fn md5_body(s: &mut [u32; 4], data: &[u8]) {
    let (mut a, mut b, mut c, mut d) = (s[0], s[1], s[2], s[3]);
    for chunk in data.as_chunks::<64>().0 {
        let x: [u32; 16] = le_words(chunk);
        let (sa, sb, sc, sd) = (a, b, c, d);
        macro_rules! step {
            ($f:expr, $a:ident, $b:ident, $c:ident, $d:ident, $x:expr, $t:expr, $s:expr) => {
                $a = $a
                    .wrapping_add($f($b, $c, $d))
                    .wrapping_add($x)
                    .wrapping_add($t);
                $a = $a.rotate_left($s).wrapping_add($b);
            };
        }
        let f = |x: u32, y: u32, z: u32| z ^ (x & (y ^ z));
        let g = |x: u32, y: u32, z: u32| y ^ (z & (x ^ y));
        let h = |x: u32, y: u32, z: u32| x ^ y ^ z;
        let i = |x: u32, y: u32, z: u32| y ^ (x | !z);
        step!(f, a, b, c, d, x[0], 0xd76aa478, 7);
        step!(f, d, a, b, c, x[1], 0xe8c7b756, 12);
        step!(f, c, d, a, b, x[2], 0x242070db, 17);
        step!(f, b, c, d, a, x[3], 0xc1bdceee, 22);
        step!(f, a, b, c, d, x[4], 0xf57c0faf, 7);
        step!(f, d, a, b, c, x[5], 0x4787c62a, 12);
        step!(f, c, d, a, b, x[6], 0xa8304613, 17);
        step!(f, b, c, d, a, x[7], 0xfd469501, 22);
        step!(f, a, b, c, d, x[8], 0x698098d8, 7);
        step!(f, d, a, b, c, x[9], 0x8b44f7af, 12);
        step!(f, c, d, a, b, x[10], 0xffff5bb1, 17);
        step!(f, b, c, d, a, x[11], 0x895cd7be, 22);
        step!(f, a, b, c, d, x[12], 0x6b901122, 7);
        step!(f, d, a, b, c, x[13], 0xfd987193, 12);
        step!(f, c, d, a, b, x[14], 0xa679438e, 17);
        step!(f, b, c, d, a, x[15], 0x49b40821, 22);

        step!(g, a, b, c, d, x[1], 0xf61e2562, 5);
        step!(g, d, a, b, c, x[6], 0xc040b340, 9);
        step!(g, c, d, a, b, x[11], 0x265e5a51, 14);
        step!(g, b, c, d, a, x[0], 0xe9b6c7aa, 20);
        step!(g, a, b, c, d, x[5], 0xd62f105d, 5);
        step!(g, d, a, b, c, x[10], 0x02441453, 9);
        step!(g, c, d, a, b, x[15], 0xd8a1e681, 14);
        step!(g, b, c, d, a, x[4], 0xe7d3fbc8, 20);
        step!(g, a, b, c, d, x[9], 0x21e1cde6, 5);
        step!(g, d, a, b, c, x[14], 0xc33707d6, 9);
        step!(g, c, d, a, b, x[3], 0xf4d50d87, 14);
        step!(g, b, c, d, a, x[8], 0x455a14ed, 20);
        step!(g, a, b, c, d, x[13], 0xa9e3e905, 5);
        step!(g, d, a, b, c, x[2], 0xfcefa3f8, 9);
        step!(g, c, d, a, b, x[7], 0x676f02d9, 14);
        step!(g, b, c, d, a, x[12], 0x8d2a4c8a, 20);

        step!(h, a, b, c, d, x[5], 0xfffa3942, 4);
        step!(h, d, a, b, c, x[8], 0x8771f681, 11);
        step!(h, c, d, a, b, x[11], 0x6d9d6122, 16);
        step!(h, b, c, d, a, x[14], 0xfde5380c, 23);
        step!(h, a, b, c, d, x[1], 0xa4beea44, 4);
        step!(h, d, a, b, c, x[4], 0x4bdecfa9, 11);
        step!(h, c, d, a, b, x[7], 0xf6bb4b60, 16);
        step!(h, b, c, d, a, x[10], 0xbebfbc70, 23);
        step!(h, a, b, c, d, x[13], 0x289b7ec6, 4);
        step!(h, d, a, b, c, x[0], 0xeaa127fa, 11);
        step!(h, c, d, a, b, x[3], 0xd4ef3085, 16);
        step!(h, b, c, d, a, x[6], 0x04881d05, 23);
        step!(h, a, b, c, d, x[9], 0xd9d4d039, 4);
        step!(h, d, a, b, c, x[12], 0xe6db99e5, 11);
        step!(h, c, d, a, b, x[15], 0x1fa27cf8, 16);
        step!(h, b, c, d, a, x[2], 0xc4ac5665, 23);

        step!(i, a, b, c, d, x[0], 0xf4292244, 6);
        step!(i, d, a, b, c, x[7], 0x432aff97, 10);
        step!(i, c, d, a, b, x[14], 0xab9423a7, 15);
        step!(i, b, c, d, a, x[5], 0xfc93a039, 21);
        step!(i, a, b, c, d, x[12], 0x655b59c3, 6);
        step!(i, d, a, b, c, x[3], 0x8f0ccc92, 10);
        step!(i, c, d, a, b, x[10], 0xffeff47d, 15);
        step!(i, b, c, d, a, x[1], 0x85845dd1, 21);
        step!(i, a, b, c, d, x[8], 0x6fa87e4f, 6);
        step!(i, d, a, b, c, x[15], 0xfe2ce6e0, 10);
        step!(i, c, d, a, b, x[6], 0xa3014314, 15);
        step!(i, b, c, d, a, x[13], 0x4e0811a1, 21);
        step!(i, a, b, c, d, x[4], 0xf7537e82, 6);
        step!(i, d, a, b, c, x[11], 0xbd3af235, 10);
        step!(i, c, d, a, b, x[2], 0x2ad7d2bb, 15);
        step!(i, b, c, d, a, x[9], 0xeb86d391, 21);

        a = a.wrapping_add(sa);
        b = b.wrapping_add(sb);
        c = c.wrapping_add(sc);
        d = d.wrapping_add(sd);
    }
    *s = [a, b, c, d];
}

impl Ctx for Md5 {
    fn update(&mut self, data: &[u8]) {
        let size = data.len();
        let saved_lo = self.lo;
        self.lo = saved_lo.wrapping_add(size as u32) & 0x1fffffff;
        if self.lo < saved_lo {
            self.hi = self.hi.wrapping_add(1);
        }
        self.hi = self.hi.wrapping_add((size >> 29) as u32);
        let used = (saved_lo & 0x3f) as usize;
        let mut data = data;
        if used != 0 {
            let free = 64 - used;
            if data.len() < free {
                self.buffer[used..used + data.len()].copy_from_slice(data);
                return;
            }
            self.buffer[used..].copy_from_slice(&data[..free]);
            data = &data[free..];
            let b = self.buffer;
            md5_body(&mut self.s, &b);
        }
        if data.len() >= 64 {
            let n = data.len() & !0x3f;
            md5_body(&mut self.s, &data[..n]);
            data = &data[n..];
        }
        self.buffer[..data.len()].copy_from_slice(data);
    }

    fn finish(&mut self, out: &mut [u8]) {
        let mut used = (self.lo & 0x3f) as usize;
        self.buffer[used] = 0x80;
        used += 1;
        let mut free = 64 - used;
        if free < 8 {
            for b in &mut self.buffer[used..] {
                *b = 0;
            }
            let b = self.buffer;
            md5_body(&mut self.s, &b);
            used = 0;
            free = 64;
        }
        for b in &mut self.buffer[used..used + free - 8] {
            *b = 0;
        }
        self.lo <<= 3;
        self.buffer[56..60].copy_from_slice(&self.lo.to_le_bytes());
        self.buffer[60..64].copy_from_slice(&self.hi.to_le_bytes());
        let b = self.buffer;
        md5_body(&mut self.s, &b);
        encode_le(&mut out[..16], &self.s);
    }

    fn image(&self) -> Vec<u8> {
        let mut v = vec![0u8; 152];
        let mut w = Img(&mut v);
        w.u32s(0, &[self.lo, self.hi]);
        w.u32s(8, &self.s);
        w.bytes(24, &self.buffer);
        w.u32s(88, &self.block);
        v
    }

    fn set_image(&mut self, img: &[u8]) {
        self.lo = rd_u32(img, 0);
        self.hi = rd_u32(img, 4);
        self.s = rd_u32s(img, 8);
        self.buffer = rd_bytes(img, 24);
        self.block = rd_u32s(img, 88);
    }
}

/// md5() helper.
pub fn md5(data: &[u8]) -> [u8; 16] {
    let mut c = Md5::new();
    c.update(data);
    let mut out = [0u8; 16];
    c.finish(&mut out);
    out
}
