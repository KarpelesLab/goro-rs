//! GMP (mpz_*) conventions on top of `puremp::Int`.
//!
//! puremp provides the arithmetic (including GMP-parity bit operations,
//! roots, Kronecker, radix 2..=62 and byte I/O); this module only maps its
//! results onto the C/PHP conventions (ULONG_MAX as -1, mpz_get_si
//! wrapping, GMP's digit validation, ...).

use puremp::{Int, Sign};
use std::fmt::Write;

/// mpz_setbit / mpz_clrbit.
pub fn set_bit(n: &Int, i: u64, value: bool) -> Int {
    let mut r = n.clone();
    if value {
        r.set_bit(i);
    } else {
        r.clear_bit(i);
    }
    r
}

/// mpz_popcount: -1 (ULONG_MAX) for negative numbers.
pub fn popcount(n: &Int) -> i64 {
    n.count_ones().map_or(-1, |c| c as i64)
}

/// mpz_hamdist: -1 (ULONG_MAX) when the signs differ.
pub fn hamdist(a: &Int, b: &Int) -> i64 {
    a.hamming_distance(b).map_or(-1, |c| c as i64)
}

/// mpz_scan0 / mpz_scan1 on the two's-complement form; -1 when there is
/// no such bit.
pub fn scan(n: &Int, start: u64, want_one: bool) -> i64 {
    let r = if want_one {
        n.scan1(start)
    } else {
        n.scan0(start)
    };
    r.map_or(-1, |v| v as i64)
}

/// mpz_get_si(): the low bits with the sign of `n`.
pub fn get_si(n: &Int) -> i64 {
    let zl = n.least_significant_limb();
    match n.sign() {
        Sign::Zero => 0,
        Sign::Positive => (zl & i64::MAX as u64) as i64,
        Sign::Negative => -1 - (zl.wrapping_sub(1) & i64::MAX as u64) as i64,
    }
}

/// mpz_get_ui(): the low limb of |n|.
pub fn get_ui(n: &Int) -> u64 {
    n.least_significant_limb()
}

pub fn digit_value(ch: u8, base: u32) -> Option<u32> {
    let d = match ch {
        b'0'..=b'9' => (ch - b'0') as u32,
        // Bases up to 36 are case-insensitive; above, A-Z are 10..35 and
        // a-z are 36..61 (mpz_set_str).
        b'A'..=b'Z' => (ch - b'A') as u32 + 10,
        b'a'..=b'z' if base <= 36 => (ch - b'a') as u32 + 10,
        b'a'..=b'z' => (ch - b'a') as u32 + 36,
        _ => return None,
    };
    (d < base).then_some(d)
}

/// Parse validated digits (no sign/prefix/whitespace) in base 2..=62.
pub fn parse_digits(digits: &[u8], base: u32) -> Option<Int> {
    // Validate with GMP's alphabet first: puremp also accepts a sign.
    if digits.is_empty() || !digits.iter().all(|&c| digit_value(c, base).is_some()) {
        return None;
    }
    let s = std::str::from_utf8(digits).ok()?;
    Int::from_str_radix(s, base).ok()
}

/// mpz_get_str() for base 2..=62 (lowercase for bases up to 36).
pub fn to_str_radix(n: &Int, base: u32) -> String {
    let mut s = String::new();
    if base == 10 {
        write!(s, "{}", n).ok();
    } else {
        n.write_radix(&mut s, base).ok();
    }
    s
}

/// mpz_probab_prime_p(): 2 = definitely prime, 1 = probably prime,
/// 0 = composite. Negative numbers are tested by absolute value.
pub fn probab_prime(n: &Int) -> i64 {
    let m = n.abs();
    if let Some(v) = m.to_u64() {
        if v <= 1_000_000 {
            return if m.is_prime_bpsw() { 2 } else { 0 };
        }
    }
    if m.is_even() || !m.is_prime_bpsw() {
        return 0;
    }
    // mpz_millerrabin() trusts Baillie-PSW below 35 * 2^46; above it says
    // "probably prime" (its extra random-base Miller-Rabin rounds only run
    // on numbers BPSW already accepted).
    if m < Int::from(35u64 << 46) { 2 } else { 1 }
}
