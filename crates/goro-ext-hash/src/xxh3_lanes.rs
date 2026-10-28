//! The XXH3 long-input inner loop (accumulate stripes, scramble at block
//! ends; xxHash specification v0.2.0, "XXH3 Algorithm Description (for
//! large inputs)"), with SSE2 and AVX2 versions of the same arithmetic
//! selected at run time.

#![allow(unsafe_code)]

use crate::engine::le64;

const P32_1: u64 = 0x9e37_79b1;

#[inline(always)]
fn accumulate_scalar(acc: &mut [u64; 8], stripe: &[u8], secret: &[u8]) {
    for i in 0..8 {
        let lane = le64(&stripe[i * 8..]);
        let v = lane ^ le64(&secret[i * 8..]);
        acc[i ^ 1] = acc[i ^ 1].wrapping_add(lane);
        acc[i] = acc[i].wrapping_add((v & 0xffff_ffff).wrapping_mul(v >> 32));
    }
}

#[inline(always)]
fn scramble_scalar(acc: &mut [u64; 8], secret: &[u8]) {
    for i in 0..8 {
        let mut a = acc[i];
        a ^= a >> 47;
        a ^= le64(&secret[i * 8..]);
        acc[i] = a.wrapping_mul(P32_1);
    }
}

/// One accumulation step with an explicit secret offset (the final stripe).
pub(crate) fn accumulate(acc: &mut [u64; 8], stripe: &[u8], secret: &[u8]) {
    accumulate_scalar(acc, stripe, secret);
}

fn stripes_scalar(acc: &mut [u64; 8], data: &[u8], secret: &[u8], mut sib: usize) -> usize {
    let spb = (secret.len() - 64) / 8;
    let sl = secret.len();
    for stripe in data.chunks(64) {
        accumulate_scalar(acc, stripe, &secret[sib * 8..]);
        sib += 1;
        if sib == spb {
            scramble_scalar(acc, &secret[sl - 64..]);
            sib = 0;
        }
    }
    sib
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    use std::arch::x86_64::*;

    /// # Safety
    /// `data` holds whole 64-byte stripes; SSE2 is part of x86_64.
    #[target_feature(enable = "sse2")]
    pub(super) unsafe fn stripes_sse2(
        acc: &mut [u64; 8],
        data: &[u8],
        secret: &[u8],
        mut sib: usize,
    ) -> usize {
        let spb = (secret.len() - 64) / 8;
        let sl = secret.len();
        // SAFETY: all loads below stay within `data` (whole stripes) and
        // `secret` (offsets below its length minus 64).
        unsafe {
            let ap = acc.as_mut_ptr() as *mut __m128i;
            let mut a = [
                _mm_loadu_si128(ap),
                _mm_loadu_si128(ap.add(1)),
                _mm_loadu_si128(ap.add(2)),
                _mm_loadu_si128(ap.add(3)),
            ];
            let prime = _mm_set1_epi32(0x9e37_79b1u32 as i32);
            let mut p = data.as_ptr();
            for _ in 0..data.len() / 64 {
                let k = secret.as_ptr().add(sib * 8);
                for (i, av) in a.iter_mut().enumerate() {
                    let d = _mm_loadu_si128(p.add(16 * i) as *const __m128i);
                    let key = _mm_loadu_si128(k.add(16 * i) as *const __m128i);
                    let dk = _mm_xor_si128(d, key);
                    let dk_hi = _mm_shuffle_epi32(dk, 0b11_11_01_01);
                    let prod = _mm_mul_epu32(dk, dk_hi);
                    let swap = _mm_shuffle_epi32(d, 0b01_00_11_10);
                    *av = _mm_add_epi64(_mm_add_epi64(*av, swap), prod);
                }
                p = p.add(64);
                sib += 1;
                if sib == spb {
                    let k = secret.as_ptr().add(sl - 64);
                    for (i, av) in a.iter_mut().enumerate() {
                        let mut x = _mm_xor_si128(*av, _mm_srli_epi64(*av, 47));
                        x = _mm_xor_si128(x, _mm_loadu_si128(k.add(16 * i) as *const __m128i));
                        let lo = _mm_mul_epu32(x, prime);
                        let hi = _mm_mul_epu32(_mm_shuffle_epi32(x, 0b11_11_01_01), prime);
                        *av = _mm_add_epi64(lo, _mm_slli_epi64(hi, 32));
                    }
                    sib = 0;
                }
            }
            for (i, av) in a.iter().enumerate() {
                _mm_storeu_si128(ap.add(i), *av);
            }
        }
        sib
    }

    /// # Safety
    /// AVX2 must be available; `data` holds whole 64-byte stripes.
    #[target_feature(enable = "avx2")]
    pub(super) unsafe fn stripes_avx2(
        acc: &mut [u64; 8],
        data: &[u8],
        secret: &[u8],
        mut sib: usize,
    ) -> usize {
        let spb = (secret.len() - 64) / 8;
        let sl = secret.len();
        // SAFETY: as in `stripes_sse2`.
        unsafe {
            let ap = acc.as_mut_ptr() as *mut __m256i;
            let mut a = [_mm256_loadu_si256(ap), _mm256_loadu_si256(ap.add(1))];
            let prime = _mm256_set1_epi32(0x9e37_79b1u32 as i32);
            let mut p = data.as_ptr();
            for _ in 0..data.len() / 64 {
                let k = secret.as_ptr().add(sib * 8);
                for (i, av) in a.iter_mut().enumerate() {
                    let d = _mm256_loadu_si256(p.add(32 * i) as *const __m256i);
                    let key = _mm256_loadu_si256(k.add(32 * i) as *const __m256i);
                    let dk = _mm256_xor_si256(d, key);
                    let dk_hi = _mm256_shuffle_epi32(dk, 0b11_11_01_01);
                    let prod = _mm256_mul_epu32(dk, dk_hi);
                    let swap = _mm256_shuffle_epi32(d, 0b01_00_11_10);
                    *av = _mm256_add_epi64(_mm256_add_epi64(*av, swap), prod);
                }
                p = p.add(64);
                sib += 1;
                if sib == spb {
                    let k = secret.as_ptr().add(sl - 64);
                    for (i, av) in a.iter_mut().enumerate() {
                        let mut x = _mm256_xor_si256(*av, _mm256_srli_epi64(*av, 47));
                        x = _mm256_xor_si256(
                            x,
                            _mm256_loadu_si256(k.add(32 * i) as *const __m256i),
                        );
                        let lo = _mm256_mul_epu32(x, prime);
                        let hi = _mm256_mul_epu32(_mm256_shuffle_epi32(x, 0b11_11_01_01), prime);
                        *av = _mm256_add_epi64(lo, _mm256_slli_epi64(hi, 32));
                    }
                    sib = 0;
                }
            }
            _mm256_storeu_si256(ap, a[0]);
            _mm256_storeu_si256(ap.add(1), a[1]);
        }
        sib
    }
}

/// Accumulates the whole stripes of `data` (a multiple of 64 bytes),
/// scrambling at each block end; `sib` is the stripe index within the
/// current block, and the new one is returned.
pub(crate) fn stripes(acc: &mut [u64; 8], data: &[u8], secret: &[u8], sib: usize) -> usize {
    debug_assert!(data.len().is_multiple_of(64));
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("avx2") {
            // SAFETY: AVX2 was detected; `data` holds whole stripes.
            return unsafe { x86::stripes_avx2(acc, data, secret, sib) };
        }
        // SAFETY: SSE2 is baseline on x86_64; `data` holds whole stripes.
        return unsafe { x86::stripes_sse2(acc, data, secret, sib) };
    }
    #[allow(unreachable_code)]
    stripes_scalar(acc, data, secret, sib)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simd_matches_scalar() {
        let secret: Vec<u8> = (0..200u32).map(|i| (i * 37 + 11) as u8).collect();
        let data: Vec<u8> = (0..64 * 40u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 7) as u8)
            .collect();
        for start in [0, 3, 16] {
            let init: [u64; 8] =
                std::array::from_fn(|i| 0x0123_4567_89ab_cdef_u64.rotate_left(i as u32 * 7));
            let mut a = init;
            let sa = stripes_scalar(&mut a, &data, &secret, start);
            #[cfg(target_arch = "x86_64")]
            {
                let mut b = init;
                // SAFETY: SSE2 is baseline on x86_64.
                let sb = unsafe { x86::stripes_sse2(&mut b, &data, &secret, start) };
                assert_eq!((a, sa), (b, sb));
                if std::is_x86_feature_detected!("avx2") {
                    let mut c = init;
                    // SAFETY: AVX2 was detected.
                    let sc = unsafe { x86::stripes_avx2(&mut c, &data, &secret, start) };
                    assert_eq!((a, sa), (c, sc));
                }
            }
        }
    }
}
