//! CRC-32 by carry-less multiplication folding (Gopal et al., "Fast CRC
//! Computation for Generic Polynomials Using PCLMULQDQ Instruction", Intel,
//! 2009), for long inputs on x86_64 CPUs with PCLMULQDQ.
//!
//! Four 128-bit accumulators fold 64 bytes at a time; they are then folded
//! into one, and the CRC of that last 128-bit value (which is congruent to
//! the message so far) is finished with the byte table. The folding
//! constants are x^n mod P, computed at compile time.

#![allow(unsafe_code)]

/// x^e mod P (P with its x^32 term), as a 32-bit remainder.
const fn xpow_mod(e: u32, poly: u64) -> u64 {
    let mut r: u64 = 1;
    let mut i = 0;
    while i < e {
        r <<= 1;
        if r & (1 << 32) != 0 {
            r ^= poly;
        }
        i += 1;
    }
    r
}

const fn reflect33(v: u64) -> u64 {
    let mut out = 0u64;
    let mut i = 0;
    while i < 33 {
        if v & (1 << i) != 0 {
            out |= 1 << (32 - i);
        }
        i += 1;
    }
    out
}

/// Folding constants (low-qword multiplier, high-qword multiplier) for a
/// distance of `d` bits.
pub(crate) const fn reflected_consts(d: u32, poly: u64) -> (u64, u64) {
    (
        reflect33(xpow_mod(d + 32, poly)),
        reflect33(xpow_mod(d - 32, poly)),
    )
}

/// (low-qword multiplier, high-qword multiplier) for MSB-first CRCs.
pub(crate) const fn normal_consts(d: u32, poly: u64) -> (u64, u64) {
    (xpow_mod(d, poly), xpow_mod(d + 64, poly))
}

/// The constants of one CRC: folding by 512 and by 128 bits.
#[derive(Clone, Copy)]
pub(crate) struct Consts {
    pub k512: (u64, u64),
    pub k128: (u64, u64),
}

pub(crate) const fn reflected(poly: u64) -> Consts {
    Consts {
        k512: reflected_consts(512, poly),
        k128: reflected_consts(128, poly),
    }
}

pub(crate) const fn normal(poly: u64) -> Consts {
    Consts {
        k512: normal_consts(512, poly),
        k128: normal_consts(128, poly),
    }
}

/// Whether the folding path can run.
#[cfg(target_arch = "x86_64")]
pub(crate) fn available() -> bool {
    std::is_x86_feature_detected!("pclmulqdq")
        && std::is_x86_feature_detected!("sse4.1")
        && std::is_x86_feature_detected!("ssse3")
}

#[cfg(not(target_arch = "x86_64"))]
pub(crate) fn available() -> bool {
    false
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    use super::Consts;
    use std::arch::x86_64::*;

    #[inline(always)]
    unsafe fn fold(x: __m128i, k: __m128i) -> __m128i {
        // SAFETY: the caller enabled pclmulqdq.
        unsafe {
            _mm_xor_si128(
                _mm_clmulepi64_si128(x, k, 0x00),
                _mm_clmulepi64_si128(x, k, 0x11),
            )
        }
    }

    /// Folds `data` (at least 64 bytes, a multiple of 16) for a reflected
    /// CRC whose register is `crc`; returns the 16 bytes left to finish
    /// with the table, starting from a zero register.
    ///
    /// # Safety
    /// PCLMULQDQ and SSE4.1 must be available.
    #[target_feature(enable = "pclmulqdq,sse4.1,ssse3")]
    pub(crate) unsafe fn fold_reflected(crc: u32, data: &[u8], c: &Consts) -> [u8; 16] {
        // SAFETY: loads stay inside `data`, whose length is a multiple of
        // 16 and at least 64.
        unsafe {
            let p = data.as_ptr() as *const __m128i;
            let n = data.len() / 16;
            let k512 = _mm_set_epi64x(c.k512.1 as i64, c.k512.0 as i64);
            let k128 = _mm_set_epi64x(c.k128.1 as i64, c.k128.0 as i64);
            let mut x = [
                _mm_xor_si128(_mm_loadu_si128(p), _mm_cvtsi32_si128(crc as i32)),
                _mm_loadu_si128(p.add(1)),
                _mm_loadu_si128(p.add(2)),
                _mm_loadu_si128(p.add(3)),
            ];
            let mut i = 4;
            while i + 4 <= n {
                for (j, xj) in x.iter_mut().enumerate() {
                    *xj = _mm_xor_si128(fold(*xj, k512), _mm_loadu_si128(p.add(i + j)));
                }
                i += 4;
            }
            let mut y = x[0];
            for xj in &x[1..] {
                y = _mm_xor_si128(fold(y, k128), *xj);
            }
            while i < n {
                y = _mm_xor_si128(fold(y, k128), _mm_loadu_si128(p.add(i)));
                i += 1;
            }
            let mut out = [0u8; 16];
            _mm_storeu_si128(out.as_mut_ptr() as *mut __m128i, y);
            out
        }
    }

    /// As `fold_reflected`, for an MSB-first CRC.
    ///
    /// # Safety
    /// PCLMULQDQ, SSSE3 and SSE4.1 must be available.
    #[target_feature(enable = "pclmulqdq,sse4.1,ssse3")]
    pub(crate) unsafe fn fold_normal(crc: u32, data: &[u8], c: &Consts) -> [u8; 16] {
        // SAFETY: as in `fold_reflected`.
        unsafe {
            let p = data.as_ptr() as *const __m128i;
            let n = data.len() / 16;
            // Byte reversal: the first message byte becomes the most
            // significant one.
            let rev = _mm_set_epi8(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15);
            let ld = |k: usize| _mm_shuffle_epi8(_mm_loadu_si128(p.add(k)), rev);
            let k512 = _mm_set_epi64x(c.k512.1 as i64, c.k512.0 as i64);
            let k128 = _mm_set_epi64x(c.k128.1 as i64, c.k128.0 as i64);
            let mut x = [
                _mm_xor_si128(ld(0), _mm_set_epi32(crc as i32, 0, 0, 0)),
                ld(1),
                ld(2),
                ld(3),
            ];
            let mut i = 4;
            while i + 4 <= n {
                for (j, xj) in x.iter_mut().enumerate() {
                    *xj = _mm_xor_si128(fold(*xj, k512), ld(i + j));
                }
                i += 4;
            }
            let mut y = x[0];
            for xj in &x[1..] {
                y = _mm_xor_si128(fold(y, k128), *xj);
            }
            while i < n {
                y = _mm_xor_si128(fold(y, k128), ld(i));
                i += 1;
            }
            let mut out = [0u8; 16];
            _mm_storeu_si128(out.as_mut_ptr() as *mut __m128i, _mm_shuffle_epi8(y, rev));
            out
        }
    }
}

#[cfg(target_arch = "x86_64")]
pub(crate) use x86::{fold_normal, fold_reflected};
