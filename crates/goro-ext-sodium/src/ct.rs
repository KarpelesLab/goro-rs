//! Constant-time helpers for comparisons of secret data.

use purecrypto::ct::ConstantTimeEq;

/// Whether `a == b`, in time depending only on the lengths.
#[inline]
pub(crate) fn eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && bool::from(a.ct_eq(b))
}

/// Little-endian comparison of two equal-length numbers: -1, 0 or 1, in
/// time depending only on the length (libsodium documentation:
/// `sodium_compare()` "runs in constant time for a given length").
pub(crate) fn compare_le(a: &[u8], b: &[u8]) -> i32 {
    debug_assert_eq!(a.len(), b.len());
    // From the most significant byte down: `gt`/`eq` masks accumulate
    // without branching on the data.
    let mut gt: u32 = 0;
    let mut lt: u32 = 0;
    let mut eq: u32 = 1;
    for i in (0..a.len()).rev() {
        let x = a[i] as u32;
        let y = b[i] as u32;
        // 1 if x > y, else 0 (both < 256, so the subtraction's sign bit says).
        let xgt = (y.wrapping_sub(x) >> 31) & 1;
        let xlt = (x.wrapping_sub(y) >> 31) & 1;
        gt |= xgt & eq;
        lt |= xlt & eq;
        // eq stays 1 while the bytes are equal.
        let ne = xgt | xlt;
        eq &= ne ^ 1;
    }
    gt as i32 - lt as i32
}

/// In-place little-endian increment (`sodium_increment()`), constant-time.
pub(crate) fn increment_le(s: &mut [u8]) {
    let mut carry: u16 = 1;
    for b in s.iter_mut() {
        carry += *b as u16;
        *b = carry as u8;
        carry >>= 8;
    }
}

/// In-place little-endian addition `a += b` modulo 2^(8·len) (`sodium_add()`),
/// constant-time. Lengths must match.
pub(crate) fn add_le(a: &mut [u8], b: &[u8]) {
    let mut carry: u16 = 0;
    for (x, y) in a.iter_mut().zip(b) {
        carry += *x as u16 + *y as u16;
        *x = carry as u8;
        carry >>= 8;
    }
}

/// Whether every byte is zero, constant-time.
pub(crate) fn is_zero(s: &[u8]) -> bool {
    let mut d = 0u8;
    for &b in s {
        d |= b;
    }
    // Fold without a data-dependent branch before the final conversion.
    ((d as u32).wrapping_sub(1) >> 8) & 1 == 1
}

/// Overwrites a buffer with zeros in a way the optimizer keeps.
pub(crate) fn wipe(buf: &mut [u8]) {
    purecrypto::zeroize::Zeroize::zeroize(buf);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare() {
        assert_eq!(compare_le(b"", b""), 0);
        assert_eq!(compare_le(b"\x01\x00", b"\x00\x01"), -1);
        assert_eq!(compare_le(b"\x00\x01", b"\x01\x00"), 1);
        assert_eq!(compare_le(b"ab", b"ab"), 0);
        assert_eq!(compare_le(b"\xff", b"\x00"), 1);
    }

    #[test]
    fn zero() {
        assert!(is_zero(&[0; 32]));
        assert!(is_zero(&[]));
        assert!(!is_zero(&[0, 0, 0x80]));
    }
}
