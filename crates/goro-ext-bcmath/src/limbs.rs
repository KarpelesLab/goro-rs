//! Natural numbers as little-endian base-10⁸ limbs (`u32`).
//!
//! Base 10⁸ matches eight ASCII digits, so conversions are a SWAR load or
//! store per limb, and no binary conversion is needed: multiplication
//! (schoolbook with deferred carries, Karatsuba above a threshold) and
//! division (Knuth's algorithm D, TAOCP vol. 2 §4.3.1) run directly in
//! decimal.

pub(crate) const BASE: u32 = 100_000_000;
const BASE64: u64 = BASE as u64;

const ASCII0: u64 = 0x3030_3030_3030_3030;

/// The SWAR core: eight ASCII digits in a little-endian word (first digit
/// in the low byte) to their value.
#[inline(always)]
fn parse8w(w: u64) -> u32 {
    let mut v = w - ASCII0;
    v = ((v & 0x0F0F_0F0F_0F0F_0F0F).wrapping_mul(2561)) >> 8;
    v = ((v & 0x00FF_00FF_00FF_00FF).wrapping_mul(6_553_601)) >> 16;
    v = ((v & 0x0000_FFFF_0000_FFFF).wrapping_mul(42_949_672_960_001)) >> 32;
    v as u32
}

/// Eight ASCII digits (most significant first) to their value.
#[inline]
pub(crate) fn parse8(chunk: &[u8]) -> u32 {
    parse8w(u64::from_le_bytes(chunk[..8].try_into().unwrap()))
}

/// A value below 10⁸ as eight ASCII digits.
#[inline]
pub(crate) fn fmt8(v: u32) -> [u8; 8] {
    let hi = v / 10_000;
    let lo = v % 10_000;
    let mut x = hi as u64 | ((lo as u64) << 32);
    let q = ((x * 5243) >> 19) & 0x0000_007F_0000_007F;
    x = q | ((x - q * 100) << 16);
    let q = ((x * 103) >> 10) & 0x000F_000F_000F_000F;
    x = q | ((x - q * 10) << 8);
    (x | ASCII0).to_le_bytes()
}

/// The value of the digits `s[start..end]` (at most eight), with one
/// overlapping 8-byte load when the slice allows it.
#[inline(always)]
fn parse_range(s: &[u8], start: usize, end: usize) -> u32 {
    let r = end - start;
    if r == 8 {
        return parse8(&s[start..end]);
    }
    if r == 0 {
        return 0;
    }
    let pad = 8 - r;
    if end >= 8 {
        // Load the eight bytes ending at `end`; blank the `pad` extra ones.
        let w = u64::from_le_bytes(s[end - 8..end].try_into().unwrap());
        let keep = !0u64 << (8 * pad);
        return parse8w((w & keep) | (ASCII0 & !keep));
    }
    if start + 8 <= s.len() {
        // Load the eight bytes starting at `start`; shift the wanted ones up.
        let w = u64::from_le_bytes(s[start..start + 8].try_into().unwrap());
        return parse8w((w << (8 * pad)) | (ASCII0 >> (8 * r)));
    }
    s[start..end]
        .iter()
        .fold(0u32, |v, &c| v * 10 + (c - b'0') as u32)
}

const POW10_U32: [u32; 9] = [
    1,
    10,
    100,
    1_000,
    10_000,
    100_000,
    1_000_000,
    10_000_000,
    100_000_000,
];

/// Number of limbs for `digits` decimal digits.
#[inline]
pub(crate) fn limb_count(digits: usize) -> usize {
    digits.div_ceil(8)
}

/// Writes the limbs of the digit string `a ++ b ++ 0*zeros` (most
/// significant first, ASCII) into `out` (at least
/// `limb_count(a.len() + b.len() + zeros)` long), least significant first;
/// returns the limb count. No copy: each limb is read from at most two
/// pieces of the operands.
pub(crate) fn from_digits_slice(a: &[u8], b: &[u8], zeros: usize, out: &mut [u32]) -> usize {
    let zero_limbs = zeros / 8;
    out[..zero_limbs].iter_mut().for_each(|x| *x = 0);
    let zrem = zeros % 8;
    let (la, lb) = (a.len(), b.len());
    let digits = la + lb;
    let total = digits + zrem;
    let nl = total.div_ceil(8);
    let mut k = zero_limbs;
    let mut end = total;
    while end > 0 {
        let start = end.saturating_sub(8);
        // Whole chunks inside one operand: one load.
        if end - start == 8 {
            if end <= la {
                out[k] = parse8(&a[start..end]);
                k += 1;
                end = start;
                continue;
            }
            if start >= la && end <= digits {
                out[k] = parse8(&b[start - la..end - la]);
                k += 1;
                end = start;
                continue;
            }
        }
        // The chunk [start, end) of the virtual string, split into its
        // a, b and trailing-zero pieces.
        let z_start = start.max(digits);
        let zl = end.saturating_sub(z_start);
        let d_end = end - zl;
        let b_start = start.max(la);
        let (bv, bl) = if d_end > b_start {
            (parse_range(b, b_start - la, d_end - la), d_end - b_start)
        } else {
            (0, 0)
        };
        let a_end = d_end.min(la);
        let av = if a_end > start {
            parse_range(a, start, a_end)
        } else {
            0
        };
        out[k] = (av * POW10_U32[bl] + bv) * POW10_U32[zl];
        k += 1;
        end = start;
    }
    debug_assert_eq!(k, zero_limbs + nl);
    k
}

/// Appends the limbs of `a ++ b ++ 0*zeros` to `out`.
pub(crate) fn from_digits(a: &[u8], b: &[u8], zeros: usize, out: &mut Vec<u32>) {
    let n = limb_count(a.len() + b.len() + zeros);
    let base = out.len();
    out.resize(base + n, 0);
    let k = from_digits_slice(a, b, zeros, &mut out[base..]);
    out.truncate(base + k);
}

/// Writes the decimal digits of `l` (no leading zeros; nothing for zero)
/// into `out` (at least `8 * l.len()` long); returns the digit count.
pub(crate) fn to_digits_slice(l: &[u32], out: &mut [u8]) -> usize {
    let mut top = l.len();
    while top > 0 && l[top - 1] == 0 {
        top -= 1;
    }
    if top == 0 {
        return 0;
    }
    let first = fmt8(l[top - 1]);
    let lz = first.iter().take_while(|&&c| c == b'0').count();
    let mut k = 8 - lz;
    out[..k].copy_from_slice(&first[lz..]);
    for &limb in l[..top - 1].iter().rev() {
        out[k..k + 8].copy_from_slice(&fmt8(limb));
        k += 8;
    }
    k
}

/// Appends the decimal digits of `l` (no leading zeros; nothing for zero).
pub(crate) fn to_digits(l: &[u32], out: &mut Vec<u8>) {
    let base = out.len();
    out.resize(base + 8 * l.len(), 0);
    let k = to_digits_slice(l, &mut out[base..]);
    out.truncate(base + k);
}

/// Removes high zero limbs.
#[inline]
pub(crate) fn normalize(v: &mut Vec<u32>) {
    while v.last() == Some(&0) {
        v.pop();
    }
}

/// Rows accumulated in `u64` before carries are propagated: each product
/// is below 10¹⁶, so 1800 of them fit next to a normalized entry.
const ROWS_PER_CARRY: usize = 1800;

fn carry_u64(acc: &mut [u64]) {
    let mut carry = 0u64;
    for x in acc.iter_mut() {
        let t = *x + carry;
        *x = t % BASE64;
        carry = t / BASE64;
    }
    debug_assert_eq!(carry, 0);
}

/// The product of two numbers of at most four limbs (zero-padded), as
/// eight limbs: fixed-size loops the compiler fully unrolls.
#[inline]
pub(crate) fn mul4(x: &[u32; 4], y: &[u32; 4]) -> [u32; 8] {
    let mut acc = [0u64; 8];
    for i in 0..4 {
        for j in 0..4 {
            acc[i + j] += x[i] as u64 * y[j] as u64;
        }
    }
    let mut out = [0u32; 8];
    let mut carry = 0u64;
    for k in 0..8 {
        let t = acc[k] + carry;
        carry = t / BASE64;
        out[k] = (t - carry * BASE64) as u32;
    }
    out
}

/// Schoolbook product into `out` (`a.len() + b.len()` limbs, overwritten).
pub(crate) fn mul_school(a: &[u32], b: &[u32], out: &mut [u32]) {
    let (a, b) = if a.len() < b.len() { (a, b) } else { (b, a) };
    let n = a.len() + b.len();
    if n <= 8 {
        let mut acc = [0u64; 8];
        mul_school_acc(a, b, &mut acc[..n], out);
    } else if n <= 32 {
        let mut acc = [0u64; 32];
        mul_school_acc(a, b, &mut acc[..n], out);
    } else {
        let mut acc = vec![0u64; n];
        mul_school_acc(a, b, &mut acc, out);
    }
}

fn mul_school_acc(a: &[u32], b: &[u32], acc: &mut [u64], out: &mut [u32]) {
    let mut rows = 0;
    for (i, &x) in a.iter().enumerate() {
        let x = x as u64;
        let dst = &mut acc[i..i + b.len()];
        for (d, &y) in dst.iter_mut().zip(b) {
            *d += x * y as u64;
        }
        rows += 1;
        if rows == ROWS_PER_CARRY {
            carry_u64(acc);
            rows = 0;
        }
    }
    carry_u64(acc);
    for (o, &v) in out.iter_mut().zip(acc.iter()) {
        *o = v as u32;
    }
}

/// Below this many limbs in the shorter operand, schoolbook beats Karatsuba.
const KARATSUBA_LIMBS: usize = 48;

/// `dst += src` (dst long enough to absorb the carry).
fn add_into(dst: &mut [u32], src: &[u32]) {
    let mut carry = 0u32;
    let mut i = 0;
    while i < src.len() {
        let t = dst[i] + src[i] + carry;
        if t >= BASE {
            dst[i] = t - BASE;
            carry = 1;
        } else {
            dst[i] = t;
            carry = 0;
        }
        i += 1;
    }
    while carry != 0 && i < dst.len() {
        let t = dst[i] + 1;
        if t >= BASE {
            dst[i] = 0;
        } else {
            dst[i] = t;
            carry = 0;
        }
        i += 1;
    }
    debug_assert_eq!(carry, 0);
}

/// `dst -= src` for `dst >= src`.
fn sub_into(dst: &mut [u32], src: &[u32]) {
    let mut borrow = 0u32;
    let mut i = 0;
    while i < src.len() {
        let s = src[i] + borrow;
        if dst[i] >= s {
            dst[i] -= s;
            borrow = 0;
        } else {
            dst[i] = dst[i] + BASE - s;
            borrow = 1;
        }
        i += 1;
    }
    while borrow != 0 && i < dst.len() {
        if dst[i] > 0 {
            dst[i] -= 1;
            borrow = 0;
        } else {
            dst[i] = BASE - 1;
        }
        i += 1;
    }
    debug_assert_eq!(borrow, 0);
}

/// `a + b` as a new vector.
fn add(a: &[u32], b: &[u32]) -> Vec<u32> {
    let (a, b) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    let mut r = Vec::with_capacity(a.len() + 1);
    r.extend_from_slice(a);
    r.push(0);
    add_into(&mut r, b);
    r
}

/// The product `a * b` (`a.len() + b.len()` limbs, possibly with high zeros).
pub(crate) fn mul(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut out = vec![0u32; a.len() + b.len()];
    if a.is_empty() || b.is_empty() {
        return out;
    }
    mul_to(a, b, &mut out);
    out
}

fn mul_to(a: &[u32], b: &[u32], out: &mut [u32]) {
    let (a, b) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    if b.len() < KARATSUBA_LIMBS {
        mul_school(a, b, out);
        return;
    }
    if a.len() >= 2 * b.len() {
        // Unbalanced: multiply b by slices of a of b's length.
        out.iter_mut().for_each(|x| *x = 0);
        let mut tmp = vec![0u32; 2 * b.len()];
        let mut i = 0;
        while i < a.len() {
            let end = (i + b.len()).min(a.len());
            let part = &a[i..end];
            let t = &mut tmp[..part.len() + b.len()];
            mul_to(part, b, t);
            add_into(&mut out[i..], t);
            i = end;
        }
        return;
    }
    // Karatsuba: a = a1*B^m + a0, b = b1*B^m + b0.
    let m = a.len() / 2;
    let (a0, a1) = a.split_at(m);
    let (b0, b1) = if b.len() > m {
        b.split_at(m)
    } else {
        (b, &[][..])
    };
    out.iter_mut().for_each(|x| *x = 0);
    let z0 = mul(a0, b0);
    let z2 = if b1.is_empty() {
        Vec::new()
    } else {
        mul(a1, b1)
    };
    let sa = add(a0, a1);
    let sb = add(b0, b1);
    let mut z1 = mul(&sa, &sb);
    sub_into(&mut z1, &z0);
    if !z2.is_empty() {
        sub_into(&mut z1, &z2);
    }
    normalize(&mut z1);
    add_into(out, trim(&z0));
    add_into(&mut out[m..], &z1);
    if !z2.is_empty() {
        add_into(&mut out[2 * m..], trim(&z2));
    }
}

pub(crate) fn trim(v: &[u32]) -> &[u32] {
    let mut n = v.len();
    while n > 0 && v[n - 1] == 0 {
        n -= 1;
    }
    &v[..n]
}

/// `a *= m` in place; returns the carry out.
fn mul_small_slice(a: &mut [u32], m: u32) -> u32 {
    let mut carry = 0u64;
    for x in a.iter_mut() {
        let t = *x as u64 * m as u64 + carry;
        *x = (t % BASE64) as u32;
        carry = t / BASE64;
    }
    carry as u32
}

/// Divides by a single limb in place, returning the remainder.
pub(crate) fn div_small_in_place(a: &mut [u32], d: u32) -> u32 {
    let mut rem = 0u64;
    let d = d as u64;
    for x in a.iter_mut().rev() {
        let cur = rem * BASE64 + *x as u64;
        *x = (cur / d) as u32;
        rem = cur % d;
    }
    rem as u32
}

/// Knuth's algorithm D on prepared operands: `u` (dividend with one extra
/// high limb, scaled by the normalization factor) becomes the scaled
/// remainder in its low `v.len()` limbs; `v` is normalized (top limb at
/// least `BASE / 2`, two limbs or more); `q` receives `u.len() - v.len()`
/// quotient limbs.
fn knuth(u: &mut [u32], v: &[u32], q: &mut [u32]) {
    let nl = v.len();
    let m = u.len() - nl - 1;
    let vt = v[nl - 1] as u64;
    let vs = v[nl - 2] as u64;
    let vt_f = vt as f64;
    for j in (0..=m).rev() {
        let num = u[j + nl] as u64 * BASE64 + u[j + nl - 1] as u64;
        // `num < 10^16 < 2^53`, so the f64 quotient is within one of the
        // exact one; fix it up with the remainder (cheaper than a 64-bit
        // hardware division).
        let mut qhat = (num as f64 / vt_f) as u64;
        let mut rhat = num as i64 - (qhat * vt) as i64;
        if rhat < 0 {
            qhat -= 1;
            rhat += vt as i64;
        } else if rhat >= vt as i64 {
            qhat += 1;
            rhat -= vt as i64;
        }
        let mut rhat = rhat as u64;
        while qhat >= BASE64 || qhat * vs > rhat * BASE64 + u[j + nl - 2] as u64 {
            qhat -= 1;
            rhat += vt;
            if rhat >= BASE64 {
                break;
            }
        }
        if qhat == 0 {
            q[j] = 0;
            continue;
        }
        // u[j..=j+nl] -= qhat * v. Each product splits into high and low
        // limbs on its own; only the borrow (0, 1 or 2) runs serially.
        let mut borrow: u64 = 0;
        let mut prev_hi: u64 = 0;
        for (ui, &vi) in u[j..j + nl].iter_mut().zip(v) {
            let p = qhat * vi as u64;
            let hi = p / BASE64;
            let lo = p - hi * BASE64;
            // t = cur + 2*BASE - (lo + prev_hi + borrow), in [1, 3*BASE)
            let t = *ui as u64 + 2 * BASE64 - lo - prev_hi - borrow;
            let b1 = (t < 2 * BASE64) as u64;
            let b2 = (t < BASE64) as u64;
            borrow = b1 + b2;
            *ui = (t + borrow * BASE64 - 2 * BASE64) as u32;
            prev_hi = hi;
        }
        let carry = prev_hi;
        let sub = carry + borrow;
        let cur = u[j + nl] as u64;
        if cur >= sub {
            u[j + nl] = (cur - sub) as u32;
        } else {
            // Went negative: add v back once.
            u[j + nl] = (cur + BASE64 - sub) as u32;
            qhat -= 1;
            let mut c = 0u32;
            for i in 0..nl {
                let s = u[j + i] + v[i] + c;
                if s >= BASE {
                    u[j + i] = s - BASE;
                    c = 1;
                } else {
                    u[j + i] = s;
                    c = 0;
                }
            }
            u[j + nl] = ((u[j + nl] as u64 + c as u64) % BASE64) as u32;
        }
        q[j] = qhat as u32;
    }
}

/// Quotient and remainder of `n / d` with caller storage: `q` gets
/// `n.len() - d.len() + 1` limbs, `r` gets `d.len()` limbs (both may have
/// high zeros); `u` and `v` are scratch of `n.len() + 1` and `d.len()`
/// limbs. `n` and `d` are trimmed, `d` non-zero, `n.len() >= d.len()`.
pub(crate) fn divrem_slices(
    n: &[u32],
    d: &[u32],
    u: &mut [u32],
    v: &mut [u32],
    q: &mut [u32],
    r: &mut [u32],
) {
    if d.len() == 1 {
        q[..n.len()].copy_from_slice(n);
        let rem = div_small_in_place(&mut q[..n.len()], d[0]);
        if let Some(r0) = r.first_mut() {
            *r0 = rem;
        }
        return;
    }
    // f = floor(BASE / (top + 1)), by an exact-enough f64 division and a fix-up.
    let top1 = d[d.len() - 1] as u64 + 1;
    let mut f = (BASE as f64 / top1 as f64) as u64;
    if f * top1 > BASE64 {
        f -= 1;
    } else if (f + 1) * top1 <= BASE64 {
        f += 1;
    }
    let f = f as u32;
    v.copy_from_slice(d);
    u[..n.len()].copy_from_slice(n);
    u[n.len()] = 0;
    if f > 1 {
        let c = mul_small_slice(v, f);
        debug_assert_eq!(c, 0);
        let c = mul_small_slice(&mut u[..n.len()], f);
        u[n.len()] = c;
    }
    knuth(u, v, q);
    // An empty `r` skips the remainder (and its un-normalizing divisions).
    if !r.is_empty() {
        let nl = d.len();
        r.copy_from_slice(&u[..nl]);
        if f > 1 {
            div_small_in_place(r, f);
        }
    }
}

/// Quotient and remainder of `n / d` (`d` non-zero). Both results may
/// carry high zero limbs.
pub(crate) fn divrem(n: &[u32], d: &[u32]) -> (Vec<u32>, Vec<u32>) {
    let d = trim(d);
    let n = trim(n);
    assert!(!d.is_empty(), "division by zero");
    if n.len() < d.len() {
        return (Vec::new(), n.to_vec());
    }
    let mut u = vec![0u32; n.len() + 1];
    let mut v = vec![0u32; d.len()];
    let mut q = vec![0u32; n.len() - d.len() + 1];
    let mut r = vec![0u32; d.len()];
    divrem_slices(n, d, &mut u, &mut v, &mut q, &mut r);
    (q, r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lim(s: &str) -> Vec<u32> {
        let mut v = Vec::new();
        from_digits(s.as_bytes(), b"", 0, &mut v);
        v
    }
    fn dig(l: &[u32]) -> String {
        let mut o = Vec::new();
        to_digits(l, &mut o);
        if o.is_empty() {
            "0".into()
        } else {
            String::from_utf8(o).unwrap()
        }
    }

    #[test]
    fn swar() {
        assert_eq!(parse8(b"12345678"), 12_345_678);
        assert_eq!(parse8(b"00000009"), 9);
        assert_eq!(&fmt8(12_345_678), b"12345678");
        assert_eq!(&fmt8(9), b"00000009");
        assert_eq!(&fmt8(99_999_999), b"99999999");
        let mut v = Vec::new();
        from_digits(b"12", b"345", 3, &mut v);
        assert_eq!(dig(&v), "12345000");
        let mut v = Vec::new();
        from_digits(b"1234567", b"89012", 11, &mut v);
        assert_eq!(dig(&v), "12345678901200000000000");
    }

    #[test]
    fn packing_every_split() {
        let src = b"9876543210123456789098765432101234567890987654321";
        for la in 0..=24 {
            for lb in 0..=24 {
                for z in 0..=19 {
                    let a = &src[..la];
                    let b = &src[la..la + lb];
                    let mut v = Vec::new();
                    from_digits(a, b, z, &mut v);
                    let mut want: String = std::str::from_utf8(&src[..la + lb]).unwrap().into();
                    want.extend(std::iter::repeat_n('0', z));
                    let want = want.trim_start_matches('0');
                    let want = if want.is_empty() { "0" } else { want };
                    assert_eq!(dig(&v), want, "la={la} lb={lb} z={z}");
                }
            }
        }
    }

    #[test]
    fn mul_div_roundtrip() {
        let a = "98765432109876543210".repeat(40);
        let b = "1234567".repeat(70);
        let p = mul(&lim(&a), &lim(&b));
        let (q, r) = divrem(&p, &lim(&b));
        assert_eq!(dig(&q), a);
        assert_eq!(dig(&r), "0");
        let (q, r) = divrem(&lim(&a), &lim("99999999999"));
        let back = mul(&q, &lim("99999999999"));
        let mut back = back;
        add_into(&mut back, &r);
        assert_eq!(dig(&back), a);
    }
}
