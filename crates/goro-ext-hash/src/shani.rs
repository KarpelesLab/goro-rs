//! SHA-1 and SHA-256 compression with the x86 SHA extensions (Intel, "Intel
//! SHA Extensions", 2013), selected at run time. The message schedule and
//! round structure are those of FIPS 180-4; the instructions compute four
//! (SHA-1) or two (SHA-256) rounds each.

#![allow(unsafe_code)]

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

/// Whether the SHA extensions (and the SSE levels used around them) are
/// available.
#[cfg(target_arch = "x86_64")]
pub(crate) fn available() -> bool {
    std::is_x86_feature_detected!("sha")
        && std::is_x86_feature_detected!("sse2")
        && std::is_x86_feature_detected!("ssse3")
        && std::is_x86_feature_detected!("sse4.1")
}

#[cfg(not(target_arch = "x86_64"))]
pub(crate) fn available() -> bool {
    false
}

#[cfg(target_arch = "x86_64")]
#[rustfmt::skip]
const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// Compresses whole 64-byte blocks into the SHA-256 state.
///
/// # Safety
/// [`available`] must have returned true.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sha,sse2,ssse3,sse4.1")]
pub(crate) unsafe fn sha256_blocks(h: &mut [u32; 8], blocks: &[[u8; 64]]) {
    let mask = _mm_set_epi64x(0x0c0d0e0f08090a0b, 0x0405060700010203);
    // SAFETY: unaligned loads and stores of the 8-word state.
    let (dcba, hgfe) = unsafe {
        (
            _mm_loadu_si128(h.as_ptr() as *const __m128i),
            _mm_loadu_si128(h.as_ptr().add(4) as *const __m128i),
        )
    };
    let cdab = _mm_shuffle_epi32(dcba, 0xb1);
    let efgh = _mm_shuffle_epi32(hgfe, 0x1b);
    let mut abef = _mm_alignr_epi8(cdab, efgh, 8);
    let mut cdgh = _mm_blend_epi16(efgh, cdab, 0xf0);
    for block in blocks {
        let (abef_save, cdgh_save) = (abef, cdgh);
        let mut m = [_mm_setzero_si128(); 4];
        for (i, mi) in m.iter_mut().enumerate() {
            // SAFETY: loads 16 of the block's 64 bytes.
            let raw = unsafe { _mm_loadu_si128(block.as_ptr().add(16 * i) as *const __m128i) };
            *mi = _mm_shuffle_epi8(raw, mask);
        }
        // Unrolled so that the schedule stays in registers.
        macro_rules! group {
            ($g:expr) => {{
                const G: usize = $g;
                if G >= 4 {
                    let t = _mm_add_epi32(
                        _mm_sha256msg1_epu32(m[G % 4], m[(G + 1) % 4]),
                        _mm_alignr_epi8(m[(G + 3) % 4], m[(G + 2) % 4], 4),
                    );
                    m[G % 4] = _mm_sha256msg2_epu32(t, m[(G + 3) % 4]);
                }
                // SAFETY: loads four round constants.
                let k = unsafe { _mm_loadu_si128(K256.as_ptr().add(4 * G) as *const __m128i) };
                let msg = _mm_add_epi32(m[G % 4], k);
                cdgh = _mm_sha256rnds2_epu32(cdgh, abef, msg);
                abef = _mm_sha256rnds2_epu32(abef, cdgh, _mm_shuffle_epi32(msg, 0x0e));
            }};
        }
        group!(0);
        group!(1);
        group!(2);
        group!(3);
        group!(4);
        group!(5);
        group!(6);
        group!(7);
        group!(8);
        group!(9);
        group!(10);
        group!(11);
        group!(12);
        group!(13);
        group!(14);
        group!(15);
        abef = _mm_add_epi32(abef, abef_save);
        cdgh = _mm_add_epi32(cdgh, cdgh_save);
    }
    let feba = _mm_shuffle_epi32(abef, 0x1b);
    let dchg = _mm_shuffle_epi32(cdgh, 0xb1);
    let dcba = _mm_blend_epi16(feba, dchg, 0xf0);
    let hgfe = _mm_alignr_epi8(dchg, feba, 8);
    // SAFETY: unaligned stores of the 8-word state.
    unsafe {
        _mm_storeu_si128(h.as_mut_ptr() as *mut __m128i, dcba);
        _mm_storeu_si128(h.as_mut_ptr().add(4) as *mut __m128i, hgfe);
    }
}

/// Compresses whole 64-byte blocks into the SHA-1 state.
///
/// # Safety
/// [`available`] must have returned true.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sha,sse2,ssse3,sse4.1")]
pub(crate) unsafe fn sha1_blocks(h: &mut [u32; 5], blocks: &[[u8; 64]]) {
    let mask = _mm_set_epi64x(0x0001020304050607, 0x08090a0b0c0d0e0f);
    // SAFETY: loads the first four state words.
    let mut abcd = unsafe { _mm_loadu_si128(h.as_ptr() as *const __m128i) };
    abcd = _mm_shuffle_epi32(abcd, 0x1b);
    let mut e0 = _mm_set_epi32(h[4] as i32, 0, 0, 0);
    for block in blocks {
        let (abcd_save, e_save) = (abcd, e0);
        let mut m = [_mm_setzero_si128(); 4];
        for (i, mi) in m.iter_mut().enumerate() {
            // SAFETY: loads 16 of the block's 64 bytes.
            let raw = unsafe { _mm_loadu_si128(block.as_ptr().add(16 * i) as *const __m128i) };
            *mi = _mm_shuffle_epi8(raw, mask);
        }
        let mut prev = abcd;
        let e = _mm_add_epi32(e0, m[0]);
        abcd = _mm_sha1rnds4_epu32(abcd, e, 0);
        // Unrolled so that the schedule stays in registers.
        macro_rules! group {
            ($g:expr) => {{
                const G: usize = $g;
                if G >= 4 {
                    let t =
                        _mm_xor_si128(_mm_sha1msg1_epu32(m[G % 4], m[(G + 1) % 4]), m[(G + 2) % 4]);
                    m[G % 4] = _mm_sha1msg2_epu32(t, m[(G + 3) % 4]);
                }
                let e = _mm_sha1nexte_epu32(prev, m[G % 4]);
                prev = abcd;
                abcd = _mm_sha1rnds4_epu32(abcd, e, (G / 5) as i32);
            }};
        }
        group!(1);
        group!(2);
        group!(3);
        group!(4);
        group!(5);
        group!(6);
        group!(7);
        group!(8);
        group!(9);
        group!(10);
        group!(11);
        group!(12);
        group!(13);
        group!(14);
        group!(15);
        group!(16);
        group!(17);
        group!(18);
        group!(19);
        e0 = _mm_sha1nexte_epu32(prev, e_save);
        abcd = _mm_add_epi32(abcd, abcd_save);
    }
    abcd = _mm_shuffle_epi32(abcd, 0x1b);
    // SAFETY: stores the first four state words.
    unsafe { _mm_storeu_si128(h.as_mut_ptr() as *mut __m128i, abcd) };
    h[4] = _mm_extract_epi32(e0, 3) as u32;
}
