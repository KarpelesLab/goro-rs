//! Decimal numbers as ASCII digit strings: parsing, formatting, and the
//! linear-time operations (comparison, addition, subtraction, rounding).
//!
//! A [`Num`] borrows the digits of a well-formed operand; a [`Dec`] owns the
//! digits of a result as an unscaled integer plus a scale. Every operation is
//! exact; results are cut to the requested scale by truncation toward zero
//! when they are formatted (PHP manual: BCMath, "the result is truncated").

use std::cmp::Ordering;

/// A well-formed bcmath number, borrowed from its string.
///
/// `int` has no leading zeros (empty for a zero integer part), `frac` is the
/// fractional digits as written (trailing zeros included, since they set a
/// `BcMath\Number`'s scale).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Num<'a> {
    pub neg: bool,
    pub int: &'a [u8],
    pub frac: &'a [u8],
}

/// Index of the first byte at or after `i` that is not an ASCII digit.
/// Eight bytes at a time: a byte is a digit when `b ^ 0x30` has a zero high
/// nibble and a low nibble below 10.
#[inline(always)]
pub(crate) fn first_nondigit(s: &[u8], mut i: usize) -> usize {
    #[inline]
    fn bad_bytes(s: &[u8], at: usize) -> u64 {
        let w = u64::from_le_bytes(s[at..at + 8].try_into().unwrap());
        let v = w ^ 0x3030_3030_3030_3030;
        (v & 0xF0F0_F0F0_F0F0_F0F0)
            | (((v & 0x0F0F_0F0F_0F0F_0F0F) + 0x0606_0606_0606_0606) & 0x1010_1010_1010_1010)
    }
    let len = s.len();
    // Long runs: 32 bytes per check.
    while i + 32 <= len {
        let any =
            bad_bytes(s, i) | bad_bytes(s, i + 8) | bad_bytes(s, i + 16) | bad_bytes(s, i + 24);
        if any != 0 {
            break;
        }
        i += 32;
    }
    while i + 8 <= len {
        let bad = bad_bytes(s, i);
        if bad != 0 {
            return i + (bad.trailing_zeros() / 8) as usize;
        }
        i += 8;
    }
    if i == len {
        return len;
    }
    if len >= 8 {
        // The last eight bytes, ignoring those before `i` (already digits).
        let at = len - 8;
        let bad = bad_bytes(s, at) & (!0u64 << (8 * (i - at)));
        return if bad != 0 {
            at + (bad.trailing_zeros() / 8) as usize
        } else {
            len
        };
    }
    while i < len && s[i].is_ascii_digit() {
        i += 1;
    }
    i
}

/// Parses a bcmath operand: an optional sign, digits, and an optional `.`
/// followed by digits. Either digit run may be empty, so `""`, `"-"`, `"."`
/// and `"+.5"` are well-formed (observed on PHP 8.5; tests
/// ext/bcmath/tests/str2num_formatting.phpt and number/construct_64bit.phpt).
/// The string ends at its first NUL byte, as PHP reads it as a C string
/// (observed on PHP 8.5: `bcadd("1\0abc", "0")` is `"1"`).
#[inline(always)]
pub(crate) fn parse(s: &[u8]) -> Option<Num<'_>> {
    let (neg, start) = match s.first() {
        Some(b'-') => (true, 1),
        Some(b'+') => (false, 1),
        _ => (false, 0),
    };
    let e = first_nondigit(s, start);
    let mut int = &s[start..e];
    let (frac, end) = if e < s.len() && s[e] == b'.' {
        let f = first_nondigit(s, e + 1);
        (&s[e + 1..f], f)
    } else {
        (&s[e..e], e)
    };
    if end < s.len() && s[end] != 0 {
        return None;
    }
    while let [b'0', rest @ ..] = int {
        int = rest;
    }
    Some(Num { neg, int, frac })
}

impl<'a> Num<'a> {
    /// The fractional digits without trailing zeros.
    #[inline]
    pub fn frac_trimmed(&self) -> &'a [u8] {
        let mut n = self.frac.len();
        while n > 0 && self.frac[n - 1] == b'0' {
            n -= 1;
        }
        &self.frac[..n]
    }

    /// The same number with trailing fractional zeros removed.
    #[inline]
    pub fn trimmed(self) -> Num<'a> {
        Num {
            frac: self.frac_trimmed(),
            ..self
        }
    }

    #[inline]
    pub fn is_zero(&self) -> bool {
        self.int.is_empty() && self.frac.iter().all(|&c| c == b'0')
    }

    /// Whether the fractional part is non-zero.
    #[inline]
    pub fn has_fraction(&self) -> bool {
        self.frac.iter().any(|&c| c != b'0')
    }

    /// Sign as -1, 0 or 1.
    #[inline]
    pub fn signum(&self) -> i32 {
        if self.is_zero() {
            0
        } else if self.neg {
            -1
        } else {
            1
        }
    }

    /// The number cut (toward zero) to at most `scale` fractional digits.
    #[inline]
    pub fn truncated(self, scale: usize) -> Num<'a> {
        if self.frac.len() > scale {
            Num {
                frac: &self.frac[..scale],
                ..self
            }
        } else {
            self
        }
    }

    #[inline]
    pub fn negated(self) -> Num<'a> {
        Num {
            neg: !self.neg,
            ..self
        }
    }

    /// Count of significant digits of the unscaled integer `int ++ frac`
    /// (leading zeros of a pure fraction excluded).
    pub fn unscaled_len(&self) -> usize {
        if !self.int.is_empty() {
            self.int.len() + self.frac.len()
        } else {
            let lz = self.frac.iter().take_while(|&&c| c == b'0').count();
            self.frac.len() - lz
        }
    }

    /// The unscaled integer's significant digits as up to two slices
    /// (`int`, then `frac`), leading zeros skipped.
    pub fn unscaled_parts(&self) -> (&'a [u8], &'a [u8]) {
        if !self.int.is_empty() {
            (self.int, self.frac)
        } else {
            let lz = self.frac.iter().take_while(|&&c| c == b'0').count();
            (&[], &self.frac[lz..])
        }
    }

    /// The integer part as an `i64` magnitude check: `Some(|int|)` when it
    /// fits a `u64`.
    pub fn int_u64(&self) -> Option<u64> {
        if self.int.len() > 20 {
            return None;
        }
        let mut v: u64 = 0;
        for &c in self.int {
            v = v.checked_mul(10)?.checked_add((c - b'0') as u64)?;
        }
        Some(v)
    }
}

/// An owned exact result: the unscaled integer's ASCII digits (possibly with
/// leading zeros, always at least `scale` long) and its scale.
#[derive(Clone, Debug, Default)]
pub(crate) struct Dec {
    pub neg: bool,
    pub digits: Vec<u8>,
    pub scale: usize,
}

impl Dec {
    pub fn zero() -> Dec {
        Dec::default()
    }

    /// Builds a result from unscaled digits; pads with leading zeros so the
    /// integer part is addressable.
    pub fn from_digits(neg: bool, mut digits: Vec<u8>, scale: usize) -> Dec {
        if digits.len() < scale {
            let pad = scale - digits.len();
            digits.splice(0..0, std::iter::repeat_n(b'0', pad));
        }
        Dec { neg, digits, scale }
    }

    pub fn view(&self) -> Num<'_> {
        let split = self.digits.len() - self.scale;
        let int = &self.digits[..split];
        let lz = int.iter().take_while(|&&c| c == b'0').count();
        Num {
            neg: self.neg,
            int: &int[lz..],
            frac: &self.digits[split..],
        }
    }
}

/// Byte length of `n` formatted at `scale`.
#[inline]
pub(crate) fn formatted_len(n: &Num, scale: usize) -> usize {
    1 + n.int.len().max(1) + if scale > 0 { scale + 1 } else { 0 }
}

/// Appends `n` cut or zero-padded to exactly `scale` fractional digits.
/// A result that is zero at that scale has no minus sign (test
/// ext/bcmath/tests/bug78238.phpt).
pub(crate) fn write_num(out: &mut Vec<u8>, n: Num, scale: usize) {
    let frac = &n.frac[..n.frac.len().min(scale)];
    let zero = n.int.is_empty() && frac.iter().all(|&c| c == b'0');
    out.reserve(formatted_len(&n, scale));
    if n.neg && !zero {
        out.push(b'-');
    }
    if n.int.is_empty() {
        out.push(b'0');
    } else {
        out.extend_from_slice(n.int);
    }
    if scale > 0 {
        out.push(b'.');
        out.extend_from_slice(frac);
        out.resize(out.len() + (scale - frac.len()), b'0');
    }
}

#[cfg(test)]
pub(crate) fn format(n: Num, scale: usize) -> Vec<u8> {
    let mut out = Vec::new();
    write_num(&mut out, n, scale);
    out
}

/// Compares magnitudes.
pub(crate) fn cmp_mag(a: &Num, b: &Num) -> Ordering {
    match a.int.len().cmp(&b.int.len()) {
        Ordering::Equal => {}
        o => return o,
    }
    match a.int.cmp(b.int) {
        Ordering::Equal => {}
        o => return o,
    }
    let n = a.frac.len().min(b.frac.len());
    match a.frac[..n].cmp(&b.frac[..n]) {
        Ordering::Equal => {}
        o => return o,
    }
    let a_rest = a.frac[n..].iter().any(|&c| c != b'0');
    let b_rest = b.frac[n..].iter().any(|&c| c != b'0');
    a_rest.cmp(&b_rest)
}

/// Compares signed values (negative zero equals zero).
pub(crate) fn cmp(a: &Num, b: &Num) -> Ordering {
    let sa = a.signum();
    let sb = b.signum();
    if sa != sb {
        return sa.cmp(&sb);
    }
    match sa {
        0 => Ordering::Equal,
        1 => cmp_mag(a, b),
        _ => cmp_mag(b, a),
    }
}

/// `dst.copy_from_slice(src)` without a library call for short lengths
/// (overlapping fixed-width moves), as digit strings are usually short.
#[inline(always)]
pub(crate) fn copy_small(dst: &mut [u8], src: &[u8]) {
    let n = src.len();
    assert_eq!(dst.len(), n);
    if (16..=32).contains(&n) {
        let a = u128::from_ne_bytes(src[..16].try_into().unwrap());
        let b = u128::from_ne_bytes(src[n - 16..].try_into().unwrap());
        dst[..16].copy_from_slice(&a.to_ne_bytes());
        dst[n - 16..].copy_from_slice(&b.to_ne_bytes());
    } else if (8..16).contains(&n) {
        let a = u64::from_ne_bytes(src[..8].try_into().unwrap());
        let b = u64::from_ne_bytes(src[n - 8..].try_into().unwrap());
        dst[..8].copy_from_slice(&a.to_ne_bytes());
        dst[n - 8..].copy_from_slice(&b.to_ne_bytes());
    } else if (4..8).contains(&n) {
        let a = u32::from_ne_bytes(src[..4].try_into().unwrap());
        let b = u32::from_ne_bytes(src[n - 4..].try_into().unwrap());
        dst[..4].copy_from_slice(&a.to_ne_bytes());
        dst[n - 4..].copy_from_slice(&b.to_ne_bytes());
    } else if n < 4 {
        for (d, s) in dst.iter_mut().zip(src) {
            *d = *s;
        }
    } else {
        dst.copy_from_slice(src);
    }
}

const ASCII0: u64 = 0x3030_3030_3030_3030;
const BIAS: u64 = 0xF6F6_F6F6_F6F6_F6F6;
const LOW_BITS: u64 = 0x0101_0101_0101_0101;

/// Turns a SWAR digit word (bytes 0..=9, or 246..=255 for a byte that
/// did not carry / did borrow) back into ASCII.
#[inline]
fn swar_fix(w: u64) -> u64 {
    (w - ((w >> 7) & LOW_BITS) * 246) + ASCII0
}

/// `dst = x + y + carry` over equal-length ASCII digit strings (most
/// significant first); returns the carry out. Eight digits per step: each
/// byte is biased by 246 so a decimal carry is a binary byte carry.
#[inline(always)]
fn swar_add(dst: &mut [u8], x: &[u8], y: &[u8], mut carry: u64) -> u64 {
    // ASCII digits: (x + 0x30) + (y + 0x30) + (BIAS - 2 * 0x30) per byte is
    // x + y + 246, carrying into the next byte exactly when x + y >= 10.
    const K: u64 = BIAS.wrapping_sub(2 * ASCII0);
    let n = dst.len();
    let (x, y) = (&x[..n], &y[..n]);
    let mut i = n;
    // Sixteen digits per step on long runs.
    const K2: u128 = (K as u128) << 64 | K as u128;
    while i >= 16 {
        i -= 16;
        let be = |s: &[u8], at: usize| u64::from_be_bytes(s[at..at + 8].try_into().unwrap());
        let a = (be(x, i) as u128) << 64 | be(x, i + 8) as u128;
        let b = (be(y, i) as u128) << 64 | be(y, i + 8) as u128;
        let (w, c1) = (a + K2).overflowing_add(b);
        let (w, c2) = w.overflowing_add(carry as u128);
        carry = (c1 | c2) as u64;
        let fix = ((w >> 7) & ((LOW_BITS as u128) << 64 | LOW_BITS as u128)) * 246;
        let res = (w - fix) + ((ASCII0 as u128) << 64 | ASCII0 as u128);
        dst[i..i + 8].copy_from_slice(&((res >> 64) as u64).to_be_bytes());
        dst[i + 8..i + 16].copy_from_slice(&(res as u64).to_be_bytes());
    }
    while i >= 8 {
        i -= 8;
        let a = u64::from_be_bytes(x[i..i + 8].try_into().unwrap());
        let b = u64::from_be_bytes(y[i..i + 8].try_into().unwrap());
        let (w, c1) = (a + K).overflowing_add(b);
        let (w, c2) = w.overflowing_add(carry);
        carry = (c1 | c2) as u64;
        dst[i..i + 8].copy_from_slice(&swar_fix(w).to_be_bytes());
    }
    let r = dst.len() % 8;
    if r > 0 && dst.len() >= 8 {
        // The leading r digits as one word over the first eight bytes; the
        // other bytes (already done) become 9 + 0, which passes a carry on.
        let head = !0u64 << (8 * (8 - r));
        let a = (u64::from_be_bytes(x[..8].try_into().unwrap()) & head)
            | (0x3939_3939_3939_3939 & !head);
        let b = (u64::from_be_bytes(y[..8].try_into().unwrap()) & head) | (ASCII0 & !head);
        let (w, c1) = (a + K).overflowing_add(b);
        let (w, c2) = w.overflowing_add(carry);
        let old = u64::from_be_bytes(dst[..8].try_into().unwrap());
        let res = (swar_fix(w) & head) | (old & !head);
        dst[..8].copy_from_slice(&res.to_be_bytes());
        return (c1 | c2) as u64;
    }
    let mut i = r;
    while i > 0 {
        i -= 1;
        let s = x[i] + y[i] - b'0' + carry as u8;
        if s > b'9' {
            dst[i] = s - 10;
            carry = 1;
        } else {
            dst[i] = s;
            carry = 0;
        }
    }
    carry
}

/// `dst = x - y - borrow` (`x` None: zeros); returns the borrow out.
#[inline(always)]
fn swar_sub(dst: &mut [u8], x: Option<&[u8]>, y: &[u8], mut borrow: u64) -> u64 {
    {
        // (x + 0x30) - (y + 0x30) per byte is x - y, borrowing from the
        // next byte exactly when x < y.
        let mut d = dst.rchunks_exact_mut(8);
        let mut ys = y.rchunks_exact(8);
        match x {
            Some(x) => {
                let mut xs = x.rchunks_exact(8);
                for ((d, xc), yc) in (&mut d).zip(&mut xs).zip(&mut ys) {
                    let a = u64::from_be_bytes(xc.try_into().unwrap());
                    let b = u64::from_be_bytes(yc.try_into().unwrap());
                    let (w, b1) = a.overflowing_sub(b);
                    let (w, b2) = w.overflowing_sub(borrow);
                    borrow = (b1 | b2) as u64;
                    d.copy_from_slice(&swar_fix(w).to_be_bytes());
                }
            }
            None => {
                for (d, yc) in (&mut d).zip(&mut ys) {
                    let b = u64::from_be_bytes(yc.try_into().unwrap()) - ASCII0;
                    let (w, b1) = 0u64.overflowing_sub(b);
                    let (w, b2) = w.overflowing_sub(borrow);
                    borrow = (b1 | b2) as u64;
                    d.copy_from_slice(&swar_fix(w).to_be_bytes());
                }
            }
        }
    }
    let r = dst.len() % 8;
    if r > 0 && dst.len() >= 8 {
        // The leading r digits as one word; the other bytes become 0 - 0,
        // which passes a borrow on.
        let head = !0u64 << (8 * (8 - r));
        let a = match x {
            Some(x) => (u64::from_be_bytes(x[..8].try_into().unwrap()) & head) | (ASCII0 & !head),
            None => ASCII0,
        };
        let b = (u64::from_be_bytes(y[..8].try_into().unwrap()) & head) | (ASCII0 & !head);
        let (w, b1) = a.overflowing_sub(b);
        let (w, b2) = w.overflowing_sub(borrow);
        let old = u64::from_be_bytes(dst[..8].try_into().unwrap());
        let res = (swar_fix(w) & head) | (old & !head);
        dst[..8].copy_from_slice(&res.to_be_bytes());
        return (b1 | b2) as u64;
    }
    let mut i = r;
    while i > 0 {
        i -= 1;
        let a = x.map_or(0, |x| (x[i] - b'0') as i8);
        let mut d = a - (y[i] - b'0') as i8 - borrow as i8;
        if d < 0 {
            d += 10;
            borrow = 1;
        } else {
            borrow = 0;
        }
        dst[i] = d as u8 + b'0';
    }
    borrow
}

/// `dst = x + carry` (carry 0 or 1).
#[inline(always)]
fn ripple_add(dst: &mut [u8], x: &[u8], carry: u64) -> u64 {
    copy_small(dst, x);
    if carry == 0 {
        return 0;
    }
    for d in dst.iter_mut().rev() {
        if *d == b'9' {
            *d = b'0';
        } else {
            *d += 1;
            return 0;
        }
    }
    1
}

/// `dst = x - borrow`.
#[inline(always)]
fn ripple_sub(dst: &mut [u8], x: &[u8], borrow: u64) -> u64 {
    copy_small(dst, x);
    if borrow == 0 {
        return 0;
    }
    for d in dst.iter_mut().rev() {
        if *d == b'0' {
            *d = b'9';
        } else {
            *d -= 1;
            return 0;
        }
    }
    1
}

/// Appends `a + b` (exact, then cut to `scale` digits) to `out`.
pub(crate) fn add_into(out: &mut Vec<u8>, a: &Num, b: &Num, scale: usize) {
    let (x, y, neg, subtract) = if a.neg == b.neg {
        (a, b, a.neg, false)
    } else if cmp_mag(a, b) == Ordering::Less {
        (b, a, b.neg, true)
    } else {
        (a, b, a.neg, true)
    };
    let il = x.int.len().max(y.int.len());
    let fl = x.frac.len().max(y.frac.len());
    // Scratch layout: [carry digit][il int digits][fl frac digits].
    let n = 1 + il + fl;
    let mut run = |buf: &mut [u8]| {
        let (ipart, fpart) = buf.split_at_mut(1 + il);
        // Fractional part, aligned at the decimal point.
        let ov = x.frac.len().min(y.frac.len());
        let mut c = 0u64;
        if !subtract {
            let longer = if x.frac.len() > ov { x.frac } else { y.frac };
            copy_small(&mut fpart[ov..], &longer[ov..]);
            c = swar_add(&mut fpart[..ov], &x.frac[..ov], &y.frac[..ov], 0);
        } else {
            if x.frac.len() > ov {
                copy_small(&mut fpart[ov..], &x.frac[ov..]);
            } else if y.frac.len() > ov {
                c = swar_sub(&mut fpart[ov..], None, &y.frac[ov..], 0);
            }
            c = swar_sub(&mut fpart[..ov], Some(&x.frac[..ov]), &y.frac[..ov], c);
        }
        // Integer part, aligned at the right; `ipart[0]` is the carry digit.
        let (xl, yl) = (x.int.len(), y.int.len());
        let ov = xl.min(yl);
        let (carry, digits) = ipart.split_at_mut(1);
        let (dhead, dlow) = digits.split_at_mut(il - ov);
        if !subtract {
            c = swar_add(dlow, &x.int[xl - ov..], &y.int[yl - ov..], c);
            let longer = if xl > yl { x.int } else { y.int };
            c = ripple_add(dhead, &longer[..il - ov], c);
            carry[0] = b'0' + c as u8;
        } else {
            c = swar_sub(dlow, Some(&x.int[xl - ov..]), &y.int[yl - ov..], c);
            let _ = ripple_sub(dhead, &x.int[..il - ov], c);
            carry[0] = b'0';
        }
        let (int, frac) = buf.split_at(1 + il);
        emit(out, neg, int, 0, frac, scale);
    };
    if n <= 64 {
        let mut buf = [0u8; 64];
        run(&mut buf[..n]);
    } else if n <= 512 {
        let mut buf = [0u8; 512];
        run(&mut buf[..n]);
    } else {
        let mut buf = vec![0u8; n];
        run(&mut buf);
    }
}

/// Appends a formatted result: the integer digits `int` (leading zeros
/// allowed) and the fraction `0 * frac_zeros ++ frac`, cut or zero-padded
/// to `scale` digits. A result that is zero at that scale has no minus sign
/// (test ext/bcmath/tests/bug78238.phpt).
pub(crate) fn emit(
    out: &mut Vec<u8>,
    neg: bool,
    int: &[u8],
    frac_zeros: usize,
    frac: &[u8],
    scale: usize,
) {
    let lz = int.iter().take_while(|&&c| c == b'0').count();
    let int = &int[lz..];
    let keep = scale.min(frac_zeros + frac.len());
    let lead = frac_zeros.min(keep);
    let fr = &frac[..keep - lead];
    let nonzero = !int.is_empty() || fr.iter().any(|&c| c != b'0');
    let sign = (neg && nonzero) as usize;
    let int_len = int.len().max(1);
    let total = sign + int_len + if scale > 0 { scale + 1 } else { 0 };
    let start = out.len();
    // One fill with zeros, then the non-zero pieces copied over it.
    out.resize(start + total, b'0');
    let dst = &mut out[start..];
    if sign == 1 {
        dst[0] = b'-';
    }
    copy_small(&mut dst[sign..sign + int.len()], int);
    if scale > 0 {
        let f = sign + int_len + 1;
        dst[f - 1] = b'.';
        copy_small(&mut dst[f + lead..f + lead + fr.len()], fr);
    }
}

/// Appends `a - b` cut to `scale` digits.
pub(crate) fn sub_into(out: &mut Vec<u8>, a: &Num, b: &Num, scale: usize) {
    add_into(out, a, &b.negated(), scale);
}

/// Appends an unscaled result to `out`: `digits` (ASCII, leading zeros
/// allowed) with `s` fractional digits, cut or padded to `scale`.
pub(crate) fn write_unscaled(out: &mut Vec<u8>, neg: bool, digits: &[u8], s: usize, scale: usize) {
    if digits.len() >= s {
        let k = digits.len() - s;
        emit(out, neg, &digits[..k], 0, &digits[k..], scale);
    } else {
        emit(out, neg, &[], s - digits.len(), digits, scale);
    }
}

/// The rounding modes of PHP's `RoundingMode` enum (PHP manual:
/// RoundingMode; RFC "Add 4 new rounding modes to round() function").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RoundingMode {
    HalfAwayFromZero,
    HalfTowardsZero,
    HalfEven,
    HalfOdd,
    TowardsZero,
    AwayFromZero,
    NegativeInfinity,
    PositiveInfinity,
}

impl RoundingMode {
    /// Every case, in declaration order (`RoundingMode::cases()`).
    pub const ALL: [RoundingMode; 8] = [
        RoundingMode::HalfAwayFromZero,
        RoundingMode::HalfTowardsZero,
        RoundingMode::HalfEven,
        RoundingMode::HalfOdd,
        RoundingMode::TowardsZero,
        RoundingMode::AwayFromZero,
        RoundingMode::NegativeInfinity,
        RoundingMode::PositiveInfinity,
    ];

    /// The case name.
    pub fn name(self) -> &'static str {
        match self {
            RoundingMode::HalfAwayFromZero => "HalfAwayFromZero",
            RoundingMode::HalfTowardsZero => "HalfTowardsZero",
            RoundingMode::HalfEven => "HalfEven",
            RoundingMode::HalfOdd => "HalfOdd",
            RoundingMode::TowardsZero => "TowardsZero",
            RoundingMode::AwayFromZero => "AwayFromZero",
            RoundingMode::NegativeInfinity => "NegativeInfinity",
            RoundingMode::PositiveInfinity => "PositiveInfinity",
        }
    }

    /// Looks a case up by name.
    pub fn from_name(name: &str) -> Option<RoundingMode> {
        RoundingMode::ALL.into_iter().find(|m| m.name() == name)
    }
}

/// Rounds `n` to `precision` decimal places (negative: to a power of ten)
/// with `mode`, returning the digits of the result at scale
/// `max(precision, 0)` (test ext/bcmath/tests/bcround_*.phpt). `max_len`
/// bounds the result length; `None` means it would exceed it.
pub(crate) fn round(n: &Num, precision: i64, mode: RoundingMode, max_len: usize) -> Option<Dec> {
    let out_scale = precision.max(0) as usize;
    if precision >= 0 && n.frac.len() <= out_scale {
        // Nothing to discard: the value at the wider scale.
        if n.int.len().saturating_add(out_scale) > max_len {
            return None;
        }
        let mut digits = Vec::with_capacity(n.int.len() + out_scale);
        digits.extend_from_slice(n.int);
        digits.extend_from_slice(n.frac);
        digits.resize(n.int.len() + out_scale, b'0');
        return Some(Dec::from_digits(n.neg, digits, out_scale));
    }
    // Split the digits into kept and discarded parts.
    let (kept_int, kept_frac, disc): (&[u8], &[u8], Vec<&[u8]>) = if precision >= 0 {
        let p = precision as usize;
        (n.int, &n.frac[..p], vec![&n.frac[p..]])
    } else {
        let drop = precision.unsigned_abs();
        if drop >= n.int.len() as u64 {
            (&[], &[], vec![n.int, n.frac])
        } else {
            let k = n.int.len() - drop as usize;
            (&n.int[..k], &[], vec![&n.int[k..], n.frac])
        }
    };
    // Classify the discarded part against one half of the last kept unit.
    let mut first: Option<u8> = None;
    let mut rest_nonzero = false;
    for part in &disc {
        for &c in part.iter() {
            if first.is_none() {
                first = Some(c - b'0');
            } else if c != b'0' {
                rest_nonzero = true;
                break;
            }
        }
        if rest_nonzero {
            break;
        }
    }
    // When the dropped digits outnumber the integer part, the first discarded
    // digit position may lie beyond them (an implicit leading zero).
    let implicit_zero = precision < 0 && precision.unsigned_abs() > n.int.len() as u64;
    let (first, rest_nonzero) = if implicit_zero {
        let any = first.is_some_and(|f| f != 0) || rest_nonzero;
        (0, any)
    } else {
        (first.unwrap_or(0), rest_nonzero)
    };
    let nonzero = first != 0 || rest_nonzero;
    let half = match first.cmp(&5) {
        Ordering::Less => Ordering::Less,
        Ordering::Greater => Ordering::Greater,
        Ordering::Equal => {
            if rest_nonzero {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        }
    };
    let last_kept = kept_frac
        .last()
        .or(kept_int.last())
        .map_or(0, |&c| c - b'0');
    let up = match mode {
        RoundingMode::HalfAwayFromZero => half != Ordering::Less,
        RoundingMode::HalfTowardsZero => half == Ordering::Greater,
        RoundingMode::HalfEven => {
            half == Ordering::Greater || (half == Ordering::Equal && last_kept % 2 == 1)
        }
        RoundingMode::HalfOdd => {
            half == Ordering::Greater || (half == Ordering::Equal && last_kept % 2 == 0)
        }
        RoundingMode::TowardsZero => false,
        RoundingMode::AwayFromZero => nonzero,
        RoundingMode::NegativeInfinity => nonzero && n.neg,
        RoundingMode::PositiveInfinity => nonzero && !n.neg,
    };
    let trailing_zeros = if precision < 0 {
        precision.unsigned_abs()
    } else {
        0
    };
    let kept_len = kept_int.len() + kept_frac.len();
    if !up && kept_int.is_empty() && kept_frac.iter().all(|&c| c == b'0') {
        // Rounded to zero: no trailing zeros needed.
        let digits = vec![b'0'; out_scale];
        return Some(Dec::from_digits(false, digits, out_scale));
    }
    let total = (kept_len as u64 + 1).saturating_add(trailing_zeros);
    if total > max_len as u64 {
        return None;
    }
    let mut digits = Vec::with_capacity(total as usize);
    digits.push(b'0');
    digits.extend_from_slice(kept_int);
    digits.extend_from_slice(kept_frac);
    if up {
        let mut i = digits.len() - 1;
        loop {
            if digits[i] == b'9' {
                digits[i] = b'0';
                i -= 1;
            } else {
                digits[i] += 1;
                break;
            }
        }
    }
    digits.resize(digits.len() + trailing_zeros as usize, b'0');
    Some(Dec::from_digits(n.neg, digits, out_scale))
}

/// Appends the decimal digits of `v` (no leading zeros; `"0"` for zero is
/// not written: an empty result means zero).
pub(crate) fn push_u128(out: &mut Vec<u8>, mut v: u128) {
    let mut buf = [0u8; 40];
    let mut i = buf.len();
    while v >= 10_000_000_000_000_000_000 {
        let low = (v % 10_000_000_000_000_000_000) as u64;
        v /= 10_000_000_000_000_000_000;
        let mut low = low;
        for _ in 0..19 {
            i -= 1;
            buf[i] = b'0' + (low % 10) as u8;
            low /= 10;
        }
    }
    let mut v = v as u64;
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    out.extend_from_slice(&buf[i..]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> Num<'_> {
        parse(s.as_bytes()).unwrap()
    }

    #[test]
    fn parse_forms() {
        for ok in ["", "-", "+", ".", "+.5", "1.", "-00.10", "0001"] {
            assert!(parse(ok.as_bytes()).is_some(), "{ok:?}");
        }
        for bad in [" 1", "1 ", "1e5", "--1", "1.2.3", "a", "1,5"] {
            assert!(parse(bad.as_bytes()).is_none(), "{bad:?}");
        }
        assert_eq!(n("0001.50").int, b"1");
        assert_eq!(n("0001.50").frac, b"50");
    }

    /// Random sums and differences against puremp's exact integers.
    #[test]
    fn add_sub_random() {
        use puremp::Int;
        use std::str::FromStr;
        let mut x: u64 = 0xDEAD_BEEF_1234_5678;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..20000 {
            let mut rnd = |max: u64| -> String {
                let n = (next() % max) as usize;
                let nines = next() % 3 == 0;
                (0..n)
                    .map(|_| {
                        let d = if nines && next() % 5 != 0 {
                            9
                        } else {
                            next() % 10
                        };
                        (b'0' + d as u8) as char
                    })
                    .collect()
            };
            let (ai, af, bi, bf) = (rnd(30), rnd(20), rnd(30), rnd(20));
            let sa = if next() % 2 == 0 { "-" } else { "" };
            let sb = if next() % 2 == 0 { "-" } else { "" };
            let a = format!("{sa}{ai}.{af}");
            let b = format!("{sb}{bi}.{bf}");
            let s = af.len().max(bf.len());
            let unscaled = |sign: &str, i: &str, f: &str| -> Int {
                let digits = format!("{i}{f}{}", "0".repeat(s - f.len()));
                let v = Int::from_str(if digits.is_empty() { "0" } else { &digits }).unwrap();
                if sign == "-" { -v } else { v }
            };
            let (ua, ub) = (unscaled(sa, &ai, &af), unscaled(sb, &bi, &bf));
            for (sum, want) in [(true, &ua + &ub), (false, &ua - &ub)] {
                let mut out = Vec::new();
                if sum {
                    add_into(&mut out, &n(&a), &n(&b), s);
                } else {
                    sub_into(&mut out, &n(&a), &n(&b), s);
                }
                let got = String::from_utf8(out).unwrap();
                // The expected value formatted at scale s.
                let neg = want < Int::from(0);
                let mag = if neg { -want.clone() } else { want.clone() }.to_string();
                let mag = format!("{}{mag}", "0".repeat((s + 1).saturating_sub(mag.len())));
                let (ip, fp) = mag.split_at(mag.len() - s);
                let mut exp = String::new();
                if neg {
                    exp.push('-');
                }
                exp.push_str(ip);
                if s > 0 {
                    exp.push('.');
                    exp.push_str(fp);
                }
                assert_eq!(got, exp, "{a} {} {b}", if sum { '+' } else { '-' });
            }
        }
    }

    #[test]
    fn add_sub() {
        let f = |a: &str, b: &str, s| {
            let mut out = Vec::new();
            add_into(&mut out, &n(a), &n(b), s);
            String::from_utf8(out).unwrap()
        };
        assert_eq!(f("1", "2", 0), "3");
        assert_eq!(f("99.99", "0.01", 2), "100.00");
        assert_eq!(f("-0.1", "0", 0), "0");
        assert_eq!(f("1", "-1.5", 1), "-0.5");
        assert_eq!(f("-1", "1.5", 1), "0.5");
        assert_eq!(f("123456789012345678.5", "-123456789012345679", 1), "-0.5");
        assert_eq!(f("99999999999999999999", "1", 0), "100000000000000000000");
        assert_eq!(
            f(
                "1000000000000000000.0000000001",
                "-999999999999999999.99999999999",
                12
            ),
            "0.000000000110"
        );
        assert_eq!(f("-0.0000000000000001", "0", 3), "0.000");
    }
}
