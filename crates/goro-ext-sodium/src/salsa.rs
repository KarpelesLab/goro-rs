//! Salsa20/20, HSalsa20 and XSalsa20 (Bernstein, "The Salsa20 family of
//! stream ciphers" and "Extending the Salsa20 nonce"), the stream under
//! `crypto_secretbox`, `crypto_box` and `crypto_stream`.
//!
//! purecrypto has no public Salsa20, so the cipher is here: a portable
//! one-block implementation and, on x86-64 with AVX2, an eight-block
//! kernel (each 256-bit register holds one state word of eight
//! consecutive blocks). Salsa20 is additions, rotations and XORs only, so
//! both are constant-time.

const SIGMA: [u32; 4] = [0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574];

#[inline(always)]
fn load32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// The 16-word input block for a key, an 8-byte nonce and a block counter.
#[inline]
fn input(key: &[u8; 32], nonce: &[u8; 8], counter: u64) -> [u32; 16] {
    let mut x = [0u32; 16];
    x[0] = SIGMA[0];
    for i in 0..4 {
        x[1 + i] = load32(&key[4 * i..]);
        x[11 + i] = load32(&key[16 + 4 * i..]);
    }
    x[5] = SIGMA[1];
    x[6] = load32(&nonce[0..]);
    x[7] = load32(&nonce[4..]);
    x[8] = counter as u32;
    x[9] = (counter >> 32) as u32;
    x[10] = SIGMA[2];
    x[15] = SIGMA[3];
    x
}

macro_rules! qr {
    ($x:ident, $a:expr, $b:expr, $c:expr, $d:expr) => {
        $x[$b] ^= $x[$a].wrapping_add($x[$d]).rotate_left(7);
        $x[$c] ^= $x[$b].wrapping_add($x[$a]).rotate_left(9);
        $x[$d] ^= $x[$c].wrapping_add($x[$b]).rotate_left(13);
        $x[$a] ^= $x[$d].wrapping_add($x[$c]).rotate_left(18);
    };
}

/// Twenty rounds (ten column/row double rounds), without the final addition.
#[inline(always)]
fn rounds(x: &mut [u32; 16]) {
    for _ in 0..10 {
        qr!(x, 0, 4, 8, 12);
        qr!(x, 5, 9, 13, 1);
        qr!(x, 10, 14, 2, 6);
        qr!(x, 15, 3, 7, 11);
        qr!(x, 0, 1, 2, 3);
        qr!(x, 5, 6, 7, 4);
        qr!(x, 10, 11, 8, 9);
        qr!(x, 15, 12, 13, 14);
    }
}

/// One 64-byte keystream block.
#[inline]
fn block(key: &[u8; 32], nonce: &[u8; 8], counter: u64) -> [u8; 64] {
    let inp = input(key, nonce, counter);
    let mut x = inp;
    rounds(&mut x);
    let mut out = [0u8; 64];
    for i in 0..16 {
        out[4 * i..4 * i + 4].copy_from_slice(&x[i].wrapping_add(inp[i]).to_le_bytes());
    }
    out
}

/// HSalsa20: a 32-byte subkey from a key and a 16-byte input.
pub fn hsalsa20(key: &[u8; 32], n: &[u8; 16]) -> [u8; 32] {
    let mut x = [0u32; 16];
    x[0] = SIGMA[0];
    for i in 0..4 {
        x[1 + i] = load32(&key[4 * i..]);
        x[11 + i] = load32(&key[16 + 4 * i..]);
        x[6 + i] = load32(&n[4 * i..]);
    }
    x[5] = SIGMA[1];
    x[10] = SIGMA[2];
    x[15] = SIGMA[3];
    rounds(&mut x);
    let mut out = [0u8; 32];
    for (i, w) in [x[0], x[5], x[10], x[15], x[6], x[7], x[8], x[9]]
        .iter()
        .enumerate()
    {
        out[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
    }
    out
}

/// XORs the Salsa20 keystream starting at block `counter` into `data`.
pub fn xor_stream(key: &[u8; 32], nonce: &[u8; 8], mut counter: u64, mut data: &mut [u8]) {
    #[cfg(target_arch = "x86_64")]
    if data.len() >= 1024 && avx512::available() {
        let n = data.len() / 1024 * 1024;
        let (bulk, rest) = data.split_at_mut(n);
        // SAFETY: AVX-512F is available (checked above).
        unsafe { avx512::xor_blocks(key, nonce, counter, bulk) };
        counter = counter.wrapping_add((n / 64) as u64);
        data = rest;
    }
    #[cfg(target_arch = "x86_64")]
    if data.len() >= 512 && avx2::available() {
        let n = data.len() / 512 * 512;
        let (bulk, rest) = data.split_at_mut(n);
        // SAFETY: AVX2 is available (checked above).
        unsafe { avx2::xor_blocks(key, nonce, counter, bulk) };
        counter = counter.wrapping_add((n / 64) as u64);
        data = rest;
    }
    while !data.is_empty() {
        let ks = block(key, nonce, counter);
        let n = data.len().min(64);
        for (d, k) in data[..n].iter_mut().zip(&ks) {
            *d ^= k;
        }
        data = &mut data[n..];
        counter = counter.wrapping_add(1);
    }
}

/// XSalsa20 subkey and 8-byte Salsa20 nonce for a 24-byte nonce.
pub fn xsalsa20_key(key: &[u8; 32], nonce: &[u8; 24]) -> ([u8; 32], [u8; 8]) {
    let sub = hsalsa20(key, nonce[..16].try_into().unwrap());
    (sub, nonce[16..].try_into().unwrap())
}

/// XORs the XSalsa20 keystream (from block `counter`) into `data`.
pub fn xsalsa20_xor(key: &[u8; 32], nonce: &[u8; 24], counter: u64, data: &mut [u8]) {
    let (mut sub, n8) = xsalsa20_key(key, nonce);
    xor_stream(&sub, &n8, counter, data);
    crate::ct::wipe(&mut sub);
}

/// The first keystream block of Salsa20 (for the Poly1305 key of
/// secretbox) followed by the XOR of `data` with the keystream from byte 32
/// on. Returns the 32-byte one-time key.
pub fn xor_after_key(key: &[u8; 32], nonce: &[u8; 8], data: &mut [u8]) -> [u8; 32] {
    let b0 = block(key, nonce, 0);
    let mut otk = [0u8; 32];
    otk.copy_from_slice(&b0[..32]);
    let n = data.len().min(32);
    for (d, k) in data[..n].iter_mut().zip(&b0[32..]) {
        *d ^= k;
    }
    if data.len() > 32 {
        xor_stream(key, nonce, 1, &mut data[32..]);
    }
    otk
}

#[cfg(target_arch = "x86_64")]
// The kernels are `unsafe fn`s only for their raw loads and stores.
#[allow(unsafe_op_in_unsafe_fn)]
mod avx2 {
    use core::arch::x86_64::*;

    pub(super) fn available() -> bool {
        std::is_x86_feature_detected!("avx2")
    }

    macro_rules! rotl {
        ($v:expr, $r:literal) => {
            _mm256_or_si256(_mm256_slli_epi32($v, $r), _mm256_srli_epi32($v, 32 - $r))
        };
    }

    macro_rules! qr {
        ($x:ident, $a:expr, $b:expr, $c:expr, $d:expr) => {
            $x[$b] = _mm256_xor_si256($x[$b], rotl!(_mm256_add_epi32($x[$a], $x[$d]), 7));
            $x[$c] = _mm256_xor_si256($x[$c], rotl!(_mm256_add_epi32($x[$b], $x[$a]), 9));
            $x[$d] = _mm256_xor_si256($x[$d], rotl!(_mm256_add_epi32($x[$c], $x[$b]), 13));
            $x[$a] = _mm256_xor_si256($x[$a], rotl!(_mm256_add_epi32($x[$d], $x[$c]), 18));
        };
    }

    /// Transposes eight vectors of eight 32-bit lanes (vector `j`, lane `i`
    /// becomes vector `i`, lane `j`).
    #[inline]
    #[target_feature(enable = "avx2")]
    fn transpose(r: [__m256i; 8]) -> [__m256i; 8] {
        let t0 = _mm256_unpacklo_epi32(r[0], r[1]);
        let t1 = _mm256_unpackhi_epi32(r[0], r[1]);
        let t2 = _mm256_unpacklo_epi32(r[2], r[3]);
        let t3 = _mm256_unpackhi_epi32(r[2], r[3]);
        let t4 = _mm256_unpacklo_epi32(r[4], r[5]);
        let t5 = _mm256_unpackhi_epi32(r[4], r[5]);
        let t6 = _mm256_unpacklo_epi32(r[6], r[7]);
        let t7 = _mm256_unpackhi_epi32(r[6], r[7]);
        let u0 = _mm256_unpacklo_epi64(t0, t2);
        let u1 = _mm256_unpackhi_epi64(t0, t2);
        let u2 = _mm256_unpacklo_epi64(t1, t3);
        let u3 = _mm256_unpackhi_epi64(t1, t3);
        let u4 = _mm256_unpacklo_epi64(t4, t6);
        let u5 = _mm256_unpackhi_epi64(t4, t6);
        let u6 = _mm256_unpacklo_epi64(t5, t7);
        let u7 = _mm256_unpackhi_epi64(t5, t7);
        [
            _mm256_permute2x128_si256(u0, u4, 0x20),
            _mm256_permute2x128_si256(u1, u5, 0x20),
            _mm256_permute2x128_si256(u2, u6, 0x20),
            _mm256_permute2x128_si256(u3, u7, 0x20),
            _mm256_permute2x128_si256(u0, u4, 0x31),
            _mm256_permute2x128_si256(u1, u5, 0x31),
            _mm256_permute2x128_si256(u2, u6, 0x31),
            _mm256_permute2x128_si256(u3, u7, 0x31),
        ]
    }

    /// XORs the keystream of blocks `counter..` into `data`, a multiple of
    /// 512 bytes.
    #[target_feature(enable = "avx2")]
    pub(super) unsafe fn xor_blocks(
        key: &[u8; 32],
        nonce: &[u8; 8],
        counter: u64,
        data: &mut [u8],
    ) {
        let inp = super::input(key, nonce, 0);
        let mut base = [_mm256_setzero_si256(); 16];
        for (b, &w) in base.iter_mut().zip(&inp) {
            *b = _mm256_set1_epi32(w as i32);
        }
        let lane = _mm256_setr_epi32(0, 1, 2, 3, 4, 5, 6, 7);
        let mut ctr = counter;
        for chunk in data.as_chunks_mut::<512>().0 {
            let mut s = base;
            // 64-bit counter per lane: low word + lane index, carry into the
            // high word.
            let lo = _mm256_add_epi32(_mm256_set1_epi32(ctr as u32 as i32), lane);
            // Carry where the low word wrapped (unsigned lo < ctr_lo).
            let bias = _mm256_set1_epi32(i32::MIN);
            let wrapped = _mm256_cmpgt_epi32(
                _mm256_xor_si256(_mm256_set1_epi32(ctr as u32 as i32), bias),
                _mm256_xor_si256(lo, bias),
            );
            let hi = _mm256_sub_epi32(_mm256_set1_epi32((ctr >> 32) as u32 as i32), wrapped);
            s[8] = lo;
            s[9] = hi;
            let orig8 = lo;
            let orig9 = hi;
            for _ in 0..10 {
                qr!(s, 0, 4, 8, 12);
                qr!(s, 5, 9, 13, 1);
                qr!(s, 10, 14, 2, 6);
                qr!(s, 15, 3, 7, 11);
                qr!(s, 0, 1, 2, 3);
                qr!(s, 5, 6, 7, 4);
                qr!(s, 10, 11, 8, 9);
                qr!(s, 15, 12, 13, 14);
            }
            for i in 0..16 {
                let o = match i {
                    8 => orig8,
                    9 => orig9,
                    _ => base[i],
                };
                s[i] = _mm256_add_epi32(s[i], o);
            }
            let lo_words = transpose([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]);
            let hi_words = transpose([s[8], s[9], s[10], s[11], s[12], s[13], s[14], s[15]]);
            let p = chunk.as_mut_ptr();
            for b in 0..8 {
                let q0 = p.add(64 * b) as *mut __m256i;
                let q1 = p.add(64 * b + 32) as *mut __m256i;
                _mm256_storeu_si256(q0, _mm256_xor_si256(_mm256_loadu_si256(q0), lo_words[b]));
                _mm256_storeu_si256(q1, _mm256_xor_si256(_mm256_loadu_si256(q1), hi_words[b]));
            }
            ctr = ctr.wrapping_add(8);
        }
    }
}

#[cfg(target_arch = "x86_64")]
// The kernel is an `unsafe fn` only for its raw loads and stores.
#[allow(unsafe_op_in_unsafe_fn)]
mod avx512 {
    use core::arch::x86_64::*;

    pub(super) fn available() -> bool {
        std::is_x86_feature_detected!("avx512f")
    }

    macro_rules! qr {
        ($x:ident, $a:expr, $b:expr, $c:expr, $d:expr) => {
            $x[$b] = _mm512_xor_si512(
                $x[$b],
                _mm512_rol_epi32(_mm512_add_epi32($x[$a], $x[$d]), 7),
            );
            $x[$c] = _mm512_xor_si512(
                $x[$c],
                _mm512_rol_epi32(_mm512_add_epi32($x[$b], $x[$a]), 9),
            );
            $x[$d] = _mm512_xor_si512(
                $x[$d],
                _mm512_rol_epi32(_mm512_add_epi32($x[$c], $x[$b]), 13),
            );
            $x[$a] = _mm512_xor_si512(
                $x[$a],
                _mm512_rol_epi32(_mm512_add_epi32($x[$d], $x[$c]), 18),
            );
        };
    }

    /// Within each 128-bit lane, turns four word-vectors into four
    /// block-vectors (lane `l` of output `i`: words of block `4l + i`).
    #[inline]
    #[target_feature(enable = "avx512f")]
    fn transpose4(r0: __m512i, r1: __m512i, r2: __m512i, r3: __m512i) -> [__m512i; 4] {
        let t0 = _mm512_unpacklo_epi32(r0, r1);
        let t1 = _mm512_unpackhi_epi32(r0, r1);
        let t2 = _mm512_unpacklo_epi32(r2, r3);
        let t3 = _mm512_unpackhi_epi32(r2, r3);
        [
            _mm512_unpacklo_epi64(t0, t2),
            _mm512_unpackhi_epi64(t0, t2),
            _mm512_unpacklo_epi64(t1, t3),
            _mm512_unpackhi_epi64(t1, t3),
        ]
    }

    /// XORs the keystream of blocks `counter..` into `data`, a multiple of
    /// 1024 bytes (sixteen blocks per pass, one block per 32-bit lane).
    #[target_feature(enable = "avx512f")]
    pub(super) unsafe fn xor_blocks(
        key: &[u8; 32],
        nonce: &[u8; 8],
        counter: u64,
        data: &mut [u8],
    ) {
        let inp = super::input(key, nonce, 0);
        let mut base = [_mm512_setzero_si512(); 16];
        for (b, &w) in base.iter_mut().zip(&inp) {
            *b = _mm512_set1_epi32(w as i32);
        }
        let lane = _mm512_setr_epi32(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15);
        let mut ctr = counter;
        for chunk in data.as_chunks_mut::<1024>().0 {
            let lo0 = _mm512_set1_epi32(ctr as u32 as i32);
            let lo = _mm512_add_epi32(lo0, lane);
            // Lanes whose low word wrapped carry into the high word.
            let wrapped = _mm512_cmplt_epu32_mask(lo, lo0);
            let hi0 = _mm512_set1_epi32((ctr >> 32) as u32 as i32);
            let hi = _mm512_mask_add_epi32(hi0, wrapped, hi0, _mm512_set1_epi32(1));
            let mut s = base;
            s[8] = lo;
            s[9] = hi;
            for _ in 0..10 {
                qr!(s, 0, 4, 8, 12);
                qr!(s, 5, 9, 13, 1);
                qr!(s, 10, 14, 2, 6);
                qr!(s, 15, 3, 7, 11);
                qr!(s, 0, 1, 2, 3);
                qr!(s, 5, 6, 7, 4);
                qr!(s, 10, 11, 8, 9);
                qr!(s, 15, 12, 13, 14);
            }
            for (i, v) in s.iter_mut().enumerate() {
                let o = match i {
                    8 => lo,
                    9 => hi,
                    _ => base[i],
                };
                *v = _mm512_add_epi32(*v, o);
            }
            let u = transpose4(s[0], s[1], s[2], s[3]);
            let v = transpose4(s[4], s[5], s[6], s[7]);
            let x = transpose4(s[8], s[9], s[10], s[11]);
            let y = transpose4(s[12], s[13], s[14], s[15]);
            let p = chunk.as_mut_ptr();
            for i in 0..4 {
                // 4×4 transpose of 128-bit lanes: block 4l + i is
                // (u[i].l, v[i].l, x[i].l, y[i].l).
                let t0 = _mm512_shuffle_i32x4(u[i], v[i], 0x44);
                let t1 = _mm512_shuffle_i32x4(u[i], v[i], 0xee);
                let t2 = _mm512_shuffle_i32x4(x[i], y[i], 0x44);
                let t3 = _mm512_shuffle_i32x4(x[i], y[i], 0xee);
                let blocks = [
                    _mm512_shuffle_i32x4(t0, t2, 0x88),
                    _mm512_shuffle_i32x4(t0, t2, 0xdd),
                    _mm512_shuffle_i32x4(t1, t3, 0x88),
                    _mm512_shuffle_i32x4(t1, t3, 0xdd),
                ];
                for (l, b) in blocks.iter().enumerate() {
                    let q = p.add(64 * (4 * l + i)) as *mut __m512i;
                    _mm512_storeu_si512(q, _mm512_xor_si512(_mm512_loadu_si512(q), *b));
                }
            }
            ctr = ctr.wrapping_add(16);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simd_matches_portable() {
        let key: [u8; 32] = core::array::from_fn(|i| i as u8 * 3 + 1);
        let nonce = [9u8, 8, 7, 6, 5, 4, 3, 2];
        for ctr in [0u64, 5, 0xffff_fffc, 0xffff_fff8, u64::MAX - 3] {
            let mut a = vec![0u8; 1024 * 3 + 512 + 100];
            xor_stream(&key, &nonce, ctr, &mut a);
            let mut b = Vec::new();
            for i in 0..(a.len().div_ceil(64)) {
                b.extend_from_slice(&block(&key, &nonce, ctr.wrapping_add(i as u64)));
            }
            b.truncate(a.len());
            assert_eq!(a, b, "counter {ctr}");
        }
    }

    #[test]
    fn hsalsa20_vector() {
        // "Cryptography in NaCl" §8 (first example): HSalsa20 of the
        // shared secret with the zero input gives the crypto_box key.
        let shared: [u8; 32] = [
            0x4a, 0x5d, 0x9d, 0x5b, 0xa4, 0xce, 0x2d, 0xe1, 0x72, 0x8e, 0x3b, 0xf4, 0x80, 0x35,
            0x0f, 0x25, 0xe0, 0x7e, 0x21, 0xc9, 0x47, 0xd1, 0x9e, 0x33, 0x76, 0xf0, 0x9b, 0x3c,
            0x1e, 0x16, 0x17, 0x42,
        ];
        let k = hsalsa20(&shared, &[0; 16]);
        assert_eq!(
            k,
            [
                0x1b, 0x27, 0x55, 0x64, 0x73, 0xe9, 0x85, 0xd4, 0x62, 0xcd, 0x51, 0x19, 0x7a, 0x9a,
                0x46, 0xc7, 0x60, 0x09, 0x54, 0x9e, 0xac, 0x64, 0x74, 0xf2, 0x06, 0xc4, 0xee, 0x08,
                0x44, 0xf6, 0x83, 0x89
            ]
        );
    }
}
