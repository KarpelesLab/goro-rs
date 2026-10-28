//! Multiplication, division, square root and powers on unscaled integers.
//!
//! Operands are the digits of `int ++ frac` (a [`Num`] with its scale).
//! Tiers by size:
//!
//! * up to 38 digits: native `u128` arithmetic;
//! * otherwise base-10⁸ limbs ([`crate::limbs`]): schoolbook/Karatsuba
//!   multiplication and Knuth division, with no binary conversion;
//! * large divisions and square roots: the `puremp` crate's sub-quadratic
//!   binary algorithms, whose decimal conversions then pay for themselves.
//!
//! Every function is exact; results are cut to the requested scale.

use crate::dec::{Dec, Num, push_u128, write_unscaled};
use crate::limbs;
use puremp::Nat;
use std::str::FromStr;

pub(crate) const POW10_U128: [u128; 39] = {
    let mut t = [1u128; 39];
    let mut i = 1;
    while i < 39 {
        t[i] = t[i - 1] * 10;
        i += 1;
    }
    t
};

/// Above this many limb products (quotient limbs × divisor limbs), division
/// goes through puremp (chosen by benchmark).
const KNUTH_MAX_WORK: usize = 200;

/// The digits `a ++ b` followed by `zeros` zeros, as a [`Nat`].
fn to_nat(a: &[u8], b: &[u8], zeros: usize) -> Nat {
    if a.is_empty() && b.is_empty() {
        return Nat::zero();
    }
    let mut s = String::with_capacity(a.len() + b.len() + zeros);
    // Digits are ASCII, validated by the parser.
    s.push_str(std::str::from_utf8(a).unwrap_or("0"));
    s.push_str(std::str::from_utf8(b).unwrap_or("0"));
    s.extend(std::iter::repeat_n('0', zeros));
    Nat::from_str(&s).unwrap_or_else(|_| Nat::zero())
}

fn nat_digits(n: &Nat) -> Vec<u8> {
    if n.is_zero() {
        return Vec::new();
    }
    n.to_string().into_bytes()
}

fn u128_digits(v: u128) -> Vec<u8> {
    let mut out = Vec::with_capacity(39);
    push_u128(&mut out, v);
    out
}

fn limb_digits(l: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    limbs::to_digits(l, &mut out);
    out
}

#[inline]
fn val_u128(a: &[u8], b: &[u8]) -> u128 {
    let mut v = 0u128;
    let mut chunks = |s: &[u8]| {
        let (chunks, rest) = s.as_chunks::<8>();
        for c in chunks {
            v = v * 100_000_000 + limbs::parse8(c) as u128;
        }
        for &c in rest {
            v = v * 10 + (c - b'0') as u128;
        }
    };
    chunks(a);
    chunks(b);
    v
}

/// Unscaled digits of `a * b` (no particular leading-zero guarantee).
fn mul_digits(a: &Num, b: &Num) -> Vec<u8> {
    let (la, lb) = (a.unscaled_len(), b.unscaled_len());
    if la == 0 || lb == 0 {
        return Vec::new();
    }
    let (a1, a2) = a.unscaled_parts();
    let (b1, b2) = b.unscaled_parts();
    if la <= 19 && lb <= 19 {
        return u128_digits(val_u128(a1, a2) * val_u128(b1, b2));
    }
    let mut x = Vec::new();
    limbs::from_digits(a1, a2, 0, &mut x);
    if a1 == b1 && a2 == b2 {
        return limb_digits(&limbs::mul(&x, &x));
    }
    let mut y = Vec::new();
    limbs::from_digits(b1, b2, 0, &mut y);
    limb_digits(&limbs::mul(&x, &y))
}

/// Operands up to this many limbs use stack buffers.
const SMALL_LIMBS: usize = 16;

/// Appends `a * b` cut to `scale` digits.
pub(crate) fn mul_into(out: &mut Vec<u8>, a: &Num, b: &Num, scale: usize) {
    let a = a.trimmed();
    let b = b.trimmed();
    let s = a.frac.len() + b.frac.len();
    let neg = a.neg != b.neg;
    let (la, lb) = (a.unscaled_len(), b.unscaled_len());
    if la > 19 && lb > 0 && la <= 32 && lb <= 32 {
        let (a1, a2) = a.unscaled_parts();
        let (b1, b2) = b.unscaled_parts();
        let mut x = [0u32; 4];
        limbs::from_digits_slice(a1, a2, 0, &mut x);
        let mut y = [0u32; 4];
        limbs::from_digits_slice(b1, b2, 0, &mut y);
        let p = limbs::mul4(&x, &y);
        let mut d = [0u8; 64];
        let k = limbs::to_digits_slice(&p, &mut d);
        write_unscaled(out, neg, &d[..k], s, scale);
        return;
    }
    if la > 19
        && lb > 0
        && limbs::limb_count(la) <= SMALL_LIMBS
        && limbs::limb_count(lb) <= SMALL_LIMBS
    {
        let (a1, a2) = a.unscaled_parts();
        let (b1, b2) = b.unscaled_parts();
        let mut x = [0u32; SMALL_LIMBS];
        let nx = limbs::from_digits_slice(a1, a2, 0, &mut x);
        let mut y = [0u32; SMALL_LIMBS];
        let ny = limbs::from_digits_slice(b1, b2, 0, &mut y);
        let mut p = [0u32; 2 * SMALL_LIMBS];
        limbs::mul_school(&x[..nx], &y[..ny], &mut p[..nx + ny]);
        let mut d = [0u8; 16 * SMALL_LIMBS];
        let k = limbs::to_digits_slice(&p[..nx + ny], &mut d);
        write_unscaled(out, neg, &d[..k], s, scale);
        return;
    }
    let d = mul_digits(&a, &b);
    write_unscaled(out, neg, &d, s, scale);
}

/// A 192-bit unsigned integer `hi * 2^128 + lo`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct U192 {
    hi: u64,
    lo: u128,
}

impl U192 {
    /// `a * b`.
    #[inline]
    fn mul(a: u128, b: u64) -> U192 {
        let p0 = (a as u64 as u128) * b as u128;
        let p1 = (a >> 64) * b as u128;
        let (lo, c) = p0.overflowing_add(p1 << 64);
        U192 {
            hi: (p1 >> 64) as u64 + c as u64,
            lo,
        }
    }
    #[inline]
    fn add(self, v: u128) -> U192 {
        let (lo, c) = self.lo.overflowing_add(v);
        U192 {
            hi: self.hi + c as u64,
            lo,
        }
    }
    #[inline]
    fn sub(self, v: U192) -> U192 {
        let (lo, b) = self.lo.overflowing_sub(v.lo);
        U192 {
            hi: self.hi - v.hi - b as u64,
            lo,
        }
    }
}

/// The value of digits `[start, end)` (at most 38) of the virtual string
/// `a ++ b ++ 0*zeros`.
fn virtual_u128(a: &[u8], b: &[u8], start: usize, end: usize) -> u128 {
    let la = a.len();
    let digits = la + b.len();
    let d_end = end.min(digits);
    let mut v = if d_end <= la {
        if start < d_end {
            val_u128(&a[start..d_end], b"")
        } else {
            0
        }
    } else if start >= la {
        if start < d_end {
            val_u128(&b[start - la..d_end - la], b"")
        } else {
            0
        }
    } else {
        val_u128(&a[start..], &b[..d_end - la])
    };
    if end > d_end {
        v *= POW10_U128[end - d_end.max(start)];
    }
    v
}

/// `N / D` for `N` of 39 to 57 digits, `D` of at most 38 and a quotient
/// below 10^15: an f64 estimate (relative error near 2^-52, so within a
/// unit or two) corrected with exact 192-bit arithmetic.
fn div_short_quotient(
    (n1, n2, nz): (&[u8], &[u8], usize),
    (d1, d2, dz): (&[u8], &[u8], usize),
) -> Option<u64> {
    let ln = n1.len() + n2.len() + nz;
    let ld = d1.len() + d2.len() + dz;
    if !(39..=57).contains(&ln) || ld > 38 || ld == 0 || ln + 1 - ld > 15 {
        return None;
    }
    let d = val_u128(d1, d2) * POW10_U128[dz];
    let hi = virtual_u128(n1, n2, 0, ln - 19);
    let lo = virtual_u128(n1, n2, ln - 19, ln) as u64;
    let n = U192::mul(hi, 10_000_000_000_000_000_000).add(lo as u128);
    let est = (hi as f64 * 1e19 + lo as f64) / d as f64;
    let mut q = est as u64;
    let d_hi = (d >> 64) as u64;
    let d_lo = d as u64;
    // q * d as U192
    let qd = |q: u64| -> U192 {
        let p0 = d_lo as u128 * q as u128;
        let p1 = d_hi as u128 * q as u128;
        U192 { hi: 0, lo: p0 }
            .add(p1 << 64)
            .add_hi((p1 >> 64) as u64)
    };
    let mut prod = qd(q);
    while prod > n {
        q -= 1;
        prod = prod.sub(U192 { hi: 0, lo: d });
    }
    let dd = U192 { hi: 0, lo: d };
    while n.sub(prod) >= dd {
        q += 1;
        prod = prod.add(d);
    }
    Some(q)
}

impl U192 {
    #[inline]
    fn add_hi(self, h: u64) -> U192 {
        U192 {
            hi: self.hi + h,
            lo: self.lo,
        }
    }
}

/// Small quotients on the stack: `Some(digits written)` when `N` and `D`
/// fit [`SMALL_LIMBS`] limbs.
fn div_small(
    (n1, n2, nz): (&[u8], &[u8], usize),
    (d1, d2, dz): (&[u8], &[u8], usize),
    qd: &mut [u8; 8 * (SMALL_LIMBS + 1)],
) -> Option<usize> {
    let ln = n1.len() + n2.len() + nz;
    let ld = d1.len() + d2.len() + dz;
    if ln == 0
        || ld == 0
        || limbs::limb_count(ln) > SMALL_LIMBS
        || limbs::limb_count(ld) > SMALL_LIMBS
    {
        return None;
    }
    if ln < ld {
        return Some(0);
    }
    let mut n = [0u32; SMALL_LIMBS];
    let nn = limbs::from_digits_slice(n1, n2, nz, &mut n);
    let mut d = [0u32; SMALL_LIMBS];
    let nd = limbs::from_digits_slice(d1, d2, dz, &mut d);
    if nn < nd {
        return Some(0);
    }
    let mut u = [0u32; SMALL_LIMBS + 1];
    let mut v = [0u32; SMALL_LIMBS];
    let mut q = [0u32; SMALL_LIMBS + 1];
    // No remainder wanted.
    limbs::divrem_slices(
        &n[..nn],
        &d[..nd],
        &mut u[..nn + 1],
        &mut v[..nd],
        &mut q[..nn - nd + 1],
        &mut [],
    );
    Some(limbs::to_digits_slice(&q[..nn - nd + 1], qd))
}

/// Integer division `N / D` with `N = digits(n1 ++ n2) * 10^nz` and
/// `D = digits(d1 ++ d2) * 10^dz`; returns quotient and remainder digits
/// (the remainder only when `want_rem`). `D` is non-zero and the slices
/// have no leading zeros.
fn divrem_digits(
    (n1, n2, nz): (&[u8], &[u8], usize),
    (d1, d2, dz): (&[u8], &[u8], usize),
    want_rem: bool,
) -> (Vec<u8>, Vec<u8>) {
    let ln = n1.len() + n2.len();
    let ld = d1.len() + d2.len();
    if ln == 0 {
        return (Vec::new(), Vec::new());
    }
    let len_n = ln + nz;
    let len_d = ld + dz;
    if len_n < len_d {
        // Quotient zero, remainder N.
        let r = if want_rem {
            let mut r = Vec::with_capacity(len_n);
            r.extend_from_slice(n1);
            r.extend_from_slice(n2);
            r.resize(len_n, b'0');
            r
        } else {
            Vec::new()
        };
        return (Vec::new(), r);
    }
    if len_n <= 38 {
        let n = val_u128(n1, n2) * POW10_U128[nz];
        let d = val_u128(d1, d2) * POW10_U128[dz];
        return (u128_digits(n / d), u128_digits(n % d));
    }
    // Common trailing zeros cancel out of the quotient (not the remainder).
    let common = if want_rem { 0 } else { nz.min(dz) };
    let (nz, dz) = (nz - common, dz - common);
    let (len_n, len_d) = (ln + nz, ld + dz);
    let q_limbs = (len_n - len_d) / 8 + 1;
    let d_limbs = len_d.div_ceil(8);
    if q_limbs.saturating_mul(d_limbs) <= KNUTH_MAX_WORK || d_limbs == 1 {
        let mut n = Vec::new();
        limbs::from_digits(n1, n2, nz, &mut n);
        let mut d = Vec::new();
        limbs::from_digits(d1, d2, dz, &mut d);
        limbs::normalize(&mut d);
        let (q, r) = limbs::divrem(&n, &d);
        return (
            limb_digits(&q),
            if want_rem {
                limb_digits(&r)
            } else {
                Vec::new()
            },
        );
    }
    let n = to_nat(n1, n2, nz);
    let d = to_nat(d1, d2, dz);
    let (q, r) = n.div_rem(&d).unwrap_or_else(|| (Nat::zero(), Nat::zero()));
    (
        nat_digits(&q),
        if want_rem { nat_digits(&r) } else { Vec::new() },
    )
}

/// The quotient digits of `a / b` at `scale` digits; `b` is non-zero.
fn div_digits(a: &Num, b: &Num, scale: usize) -> Vec<u8> {
    let (sa, sb) = (a.frac.len() as i128, b.frac.len() as i128);
    // q = trunc(A * 10^(sb + scale - sa) / B)
    let e = sb + scale as i128 - sa;
    let (nz, dz) = if e >= 0 {
        (e as usize, 0)
    } else {
        (0, (-e) as usize)
    };
    let (a1, a2) = a.unscaled_parts();
    let (b1, b2) = b.unscaled_parts();
    divrem_digits((a1, a2, nz), (b1, b2, dz), false).0
}

/// Appends `a / b` truncated at `scale` digits; `b` is non-zero.
pub(crate) fn div_into(out: &mut Vec<u8>, a: &Num, b: &Num, scale: usize) {
    let a = a.trimmed();
    let b = b.trimmed();
    let (sa, sb) = (a.frac.len() as i128, b.frac.len() as i128);
    let e = sb + scale as i128 - sa;
    let (nz, dz) = if e >= 0 {
        (e as usize, 0)
    } else {
        (0, (-e) as usize)
    };
    let (a1, a2) = a.unscaled_parts();
    let (b1, b2) = b.unscaled_parts();
    let ln = a1.len() + a2.len() + nz;
    if ln > 38 {
        if let Some(q) = div_short_quotient((a1, a2, nz), (b1, b2, dz)) {
            let mut d = [0u8; 20];
            let mut v = q;
            let mut k = d.len();
            while v > 0 {
                k -= 1;
                d[k] = b'0' + (v % 10) as u8;
                v /= 10;
            }
            write_unscaled(out, a.neg != b.neg, &d[k..], scale, scale);
            return;
        }
        let mut qd = [0u8; 8 * (SMALL_LIMBS + 1)];
        if let Some(k) = div_small((a1, a2, nz), (b1, b2, dz), &mut qd) {
            write_unscaled(out, a.neg != b.neg, &qd[..k], scale, scale);
            return;
        }
    }
    let q = div_digits(&a, &b, scale);
    write_unscaled(out, a.neg != b.neg, &q, scale, scale);
}

/// `a / b` truncated at `scale` digits; `b` is non-zero.
pub(crate) fn div(a: &Num, b: &Num, scale: usize) -> Dec {
    let a = a.trimmed();
    let b = b.trimmed();
    let q = div_digits(&a, &b, scale);
    Dec::from_digits(a.neg != b.neg, q, scale)
}

/// Truncated division: integer quotient and exact remainder (scale
/// `max(a.frac, b.frac)`, sign of `a`); `b` is non-zero.
pub(crate) fn divmod(a: &Num, b: &Num) -> (Dec, Dec) {
    let a = a.trimmed();
    let b = b.trimmed();
    let (sa, sb) = (a.frac.len(), b.frac.len());
    let s = sa.max(sb);
    let (a1, a2) = a.unscaled_parts();
    let (b1, b2) = b.unscaled_parts();
    let (q, r) = divrem_digits((a1, a2, s - sa), (b1, b2, s - sb), true);
    (
        Dec::from_digits(a.neg != b.neg, q, 0),
        Dec::from_digits(a.neg, r, s),
    )
}

/// `sqrt(a)` truncated at `scale` digits; `a >= 0`.
pub(crate) fn sqrt(a: &Num, scale: usize) -> Dec {
    let a = a.trimmed();
    let sa = a.frac.len();
    let (a1, a2) = a.unscaled_parts();
    if a1.is_empty() && a2.is_empty() {
        return Dec::from_digits(false, Vec::new(), scale);
    }
    // floor(sqrt(A * 10^(2*scale - sa)))
    let shift = 2 * scale as i128 - sa as i128;
    let (x1, x2, zeros): (&[u8], &[u8], usize) = if shift >= 0 {
        (a1, a2, shift as usize)
    } else {
        // Drop the lowest -shift digits: floor(sqrt(floor(x))) == floor(sqrt(x)).
        let drop = (-shift) as usize;
        let total = a1.len() + a2.len();
        if drop >= total {
            return Dec::from_digits(false, Vec::new(), scale);
        }
        let keep = total - drop;
        if keep <= a1.len() {
            (&a1[..keep], &[][..], 0)
        } else {
            (a1, &a2[..keep - a1.len()], 0)
        }
    };
    let len = x1.len() + x2.len() + zeros;
    let digits = if len <= 38 {
        let v = val_u128(x1, x2) * POW10_U128[zeros];
        let r = Nat::from_u128(v).isqrt().to_u128().unwrap_or(0);
        u128_digits(r)
    } else {
        nat_digits(&to_nat(x1, x2, zeros).isqrt())
    };
    Dec::from_digits(false, digits, scale)
}

/// Estimated digit count of `a^e` (an upper bound).
pub(crate) fn pow_len_estimate(a: &Num, e: u64) -> u128 {
    a.trimmed().unscaled_len() as u128 * e as u128
}

/// Exact `a^e` for `e >= 1` (scale `a.frac * e` of the trimmed operand).
/// The caller bounds the size (see [`pow_len_estimate`]).
pub(crate) fn pow(a: &Num, e: u64) -> Dec {
    let a = a.trimmed();
    let scale = a.frac.len() * e as usize;
    let neg = a.neg && e % 2 == 1;
    let la = a.unscaled_len();
    if la == 0 {
        return Dec::from_digits(false, Vec::new(), scale);
    }
    let (a1, a2) = a.unscaled_parts();
    let est = la as u128 * e as u128;
    let digits = if est <= 38 {
        let b = val_u128(a1, a2);
        let mut r = 1u128;
        for _ in 0..e {
            r *= b;
        }
        u128_digits(r)
    } else {
        let mut base = Vec::new();
        limbs::from_digits(a1, a2, 0, &mut base);
        let mut result: Option<Vec<u32>> = None;
        let mut k = e;
        loop {
            if k & 1 == 1 {
                result = Some(match result {
                    None => base.clone(),
                    Some(r) => {
                        let mut p = limbs::mul(&r, &base);
                        limbs::normalize(&mut p);
                        p
                    }
                });
            }
            k >>= 1;
            if k == 0 {
                break;
            }
            let mut sq = limbs::mul(&base, &base);
            limbs::normalize(&mut sq);
            base = sq;
        }
        limb_digits(&result.unwrap_or_default())
    };
    Dec::from_digits(neg, digits, scale)
}

/// The magnitude of an integer-valued operand as a [`Nat`].
fn int_nat(a: &Num) -> Nat {
    to_nat(a.int, b"", 0)
}

/// `|base|^exp mod |modulus|` with the sign of `base` when `exp` is odd
/// (truncated modulo, as `bcpowmod("-2", "5", "7")` is `"-4"`: test
/// ext/bcmath/tests/bcpowmod.phpt). All three are integer-valued, `exp >= 0`
/// and the modulus non-zero.
pub(crate) fn powmod(base: &Num, exp: &Num, modulus: &Num) -> Dec {
    let m_digits = modulus.int;
    let odd_exp = exp.int.last().is_some_and(|&c| (c - b'0') % 2 == 1);
    let neg = base.neg && odd_exp;
    let digits = if m_digits.len() <= 19 && exp.int.len() <= 38 {
        let m = val_u128(m_digits, b"") as u64;
        if m == 1 {
            Vec::new()
        } else {
            let m128 = m as u128;
            let b = if base.int.len() <= 38 {
                val_u128(base.int, b"") % m128
            } else {
                let mut l = Vec::new();
                limbs::from_digits(base.int, b"", 0, &mut l);
                let mut ml = Vec::new();
                limbs::from_digits(m_digits, b"", 0, &mut ml);
                limbs::normalize(&mut ml);
                let (_, r) = limbs::divrem(&l, &ml);
                val_u128(&limb_digits(&r), b"")
            };
            let mut e = val_u128(exp.int, b"");
            let mut result: u128 = 1 % m128;
            let mut sq = b;
            while e > 0 {
                if e & 1 == 1 {
                    result = result * sq % m128;
                }
                e >>= 1;
                if e > 0 {
                    sq = sq * sq % m128;
                }
            }
            u128_digits(result)
        }
    } else {
        let m = int_nat(modulus);
        let r = int_nat(base).modpow(&int_nat(exp), &m);
        nat_digits(&r)
    };
    let neg = neg && !digits.is_empty();
    Dec::from_digits(neg, digits, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dec::{format, parse};

    fn s(d: &Dec, scale: usize) -> String {
        String::from_utf8(format(d.view(), scale)).unwrap()
    }
    fn n(x: &str) -> Num<'_> {
        parse(x.as_bytes()).unwrap()
    }
    fn m(a: &str, b: &str, scale: usize) -> String {
        let mut out = Vec::new();
        mul_into(&mut out, &n(a), &n(b), scale);
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn basic() {
        assert_eq!(m("1.5", "-2", 2), "-3.00");
        assert_eq!(m("-0.5", "0.2", 0), "0");
        assert_eq!(s(&div(&n("1"), &n("3"), 5), 5), "0.33333");
        assert_eq!(s(&div(&n("100"), &n("1.1"), 1), 1), "90.9");
        let (q, r) = divmod(&n("-5.7"), &n("2"));
        assert_eq!(s(&q, 0), "-2");
        assert_eq!(s(&r, 3), "-1.700");
        assert_eq!(s(&sqrt(&n("2"), 10), 10), "1.4142135623");
        assert_eq!(s(&pow(&n("1.5"), 3), 3), "3.375");
        assert_eq!(s(&powmod(&n("-2"), &n("5"), &n("7")), 0), "-4");
    }

    /// Random operands (with runs of 9s and 0s, which stress the quotient
    /// digit estimate) against puremp.
    #[test]
    fn random_against_puremp() {
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let digits = |n: usize, next: &mut dyn FnMut() -> u64| -> String {
            let style = next() % 4;
            let mut s = String::with_capacity(n);
            for i in 0..n {
                let d = match style {
                    0 => next() % 10,
                    1 => {
                        if next().is_multiple_of(8) {
                            next() % 10
                        } else {
                            9
                        }
                    }
                    2 => {
                        if next().is_multiple_of(8) {
                            next() % 10
                        } else {
                            0
                        }
                    }
                    _ => [0, 9, 1, 8][(next() % 4) as usize],
                };
                let d = if i == 0 && d == 0 { 1 } else { d };
                s.push((b'0' + d as u8) as char);
            }
            s
        };
        for i in 0..3000 {
            // Every 40th case is large enough for Karatsuba and puremp.
            let (ma, mb) = if i % 40 == 0 { (3000, 2000) } else { (140, 90) };
            let la = 1 + (next() % ma) as usize;
            let lb = 1 + (next() % mb) as usize;
            let a = digits(la, &mut next);
            let b = digits(lb, &mut next);
            let na = to_nat(a.as_bytes(), b"", 0);
            let nb = to_nat(b.as_bytes(), b"", 0);
            let (wq, wr) = na.div_rem(&nb).unwrap();
            let (q, r) = divmod(&n(&a), &n(&b));
            assert_eq!(q.view().int, nat_digits(&wq).as_slice(), "{a} / {b}");
            assert_eq!(r.view().int, nat_digits(&wr).as_slice(), "{a} % {b}");
            let p = m(&a, &b, 0);
            assert_eq!(
                p.as_bytes(),
                nat_digits(&na.mul(&nb)).as_slice(),
                "{a} * {b}"
            );
        }
    }

    /// The quotient fast paths of `div_into` against the general `div`.
    #[test]
    fn div_fast_paths() {
        let mut x: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let digits = |n: usize, next: &mut dyn FnMut() -> u64| -> String {
            let nines = next().is_multiple_of(3);
            (0..n)
                .map(|i| {
                    let d = if nines && !next().is_multiple_of(4) {
                        9
                    } else {
                        next() % 10
                    };
                    let d = if i == 0 && d == 0 { 1 } else { d };
                    (b'0' + d as u8) as char
                })
                .collect()
        };
        for _ in 0..20000 {
            let ai = digits(1 + (next() % 30) as usize, &mut next);
            let af = digits((next() % 25) as usize, &mut next);
            let bi = digits(1 + (next() % 25) as usize, &mut next);
            let bf = digits((next() % 15) as usize, &mut next);
            let a = format!("{ai}.{af}");
            let b = if next() % 2 == 0 {
                format!("-{bi}.{bf}")
            } else {
                format!("{bi}.{bf}")
            };
            let scale = (next() % 25) as usize;
            let mut fast = Vec::new();
            div_into(&mut fast, &n(&a), &n(&b), scale);
            let slow = s(&div(&n(&a), &n(&b), scale), scale);
            assert_eq!(
                String::from_utf8(fast).unwrap(),
                slow,
                "{a} / {b} scale {scale}"
            );
        }
    }

    #[test]
    fn tiers_agree() {
        // Limb and puremp paths against each other.
        for len in [30usize, 100, 700, 3000] {
            let a: String = (0..len)
                .map(|i| (b'1' + (i * 7 % 9) as u8) as char)
                .collect();
            let b: String = (0..len / 2 + 3)
                .map(|i| (b'1' + (i * 5 % 9) as u8) as char)
                .collect();
            let p = m(&a, &b, 0);
            let want = to_nat(a.as_bytes(), b"", 0).mul(&to_nat(b.as_bytes(), b"", 0));
            assert_eq!(p.as_bytes(), nat_digits(&want).as_slice(), "mul {len}");
            let q = div(&n(&a), &n(&b), 0);
            let (wq, _) = to_nat(a.as_bytes(), b"", 0)
                .div_rem(&to_nat(b.as_bytes(), b"", 0))
                .unwrap();
            assert_eq!(q.view().int, nat_digits(&wq).as_slice(), "div {len}");
        }
    }
}
