//! Float to string conversion, as PHP 8.5 renders floats.
//!
//! Three layers:
//!
//! - [`shortest`]: the shortest decimal digit string that reads back as the
//!   same double under round-to-nearest-even parsing, choosing the candidate
//!   closest to the exact binary value when several shortest strings exist.
//!   This is the output of Steele & White / Burger & Dybvig ("Printing
//!   Floating-Point Numbers Quickly and Accurately", 1996), Ryu and
//!   Schubfach. It is computed here with the Schubfach method (R. Giulietti,
//!   "The Schubfach way to render doubles", 2020): the rounding interval of
//!   the double is scaled by a power of ten so that it is between 1 and 10
//!   units wide, and the few integer candidates inside it are compared with
//!   round-to-odd 126-bit approximations of the scaled bounds. The table of
//!   126-bit powers of ten is computed from its definition with the exact
//!   bignum below, on first use.
//! - [`fixed_digits`]: the first `n` significant digits of the exact binary
//!   value, correctly rounded with exact ties going to the even digit
//!   (observed on PHP 8.5: with `precision=1`, 1.5 gives "2", 2.5 gives "2",
//!   0.125 gives "0.1"). Exact digit generation with a bignum (the "fixed
//!   format" case of Steele & White's Dragon4), with a shortcut through the
//!   shortest digits when that is provably the same answer.
//! - [`format_php`]: PHP's layout of those digits for a given ini
//!   `precision` (observed on PHP 8.5: `echo`/`(string)` use `precision`,
//!   14 by default; `-1` means shortest round-trip with a 17-digit
//!   threshold for the exponential form, as `var_dump`/`json_encode` use with
//!   `serialize_precision=-1`).
//!
//! Exponent convention used throughout: digits `d1 d2 d3 ...` (ASCII, no
//! leading or trailing zeros) and exponent `x` mean `d1.d2d3... × 10^x`, so
//! `x` is the exponent PHP prints in its `E` form.

use std::cmp::Ordering;
use std::sync::OnceLock;

/// Room for every digit [`fixed_digits`] can produce: the exact decimal
/// expansion of a finite double has at most 767 significant digits.
pub const MAX_DIGITS: usize = 800;

/// Digits returned by [`shortest`] and [`fixed_digits`]: ASCII `'0'..='9'`,
/// first digit nonzero, no trailing zeros.
#[derive(Clone)]
pub struct Digits {
    len: usize,
    buf: [u8; MAX_DIGITS],
}

impl Digits {
    fn from_slice(d: &[u8]) -> Digits {
        let mut buf = [0u8; MAX_DIGITS];
        buf[..d.len()].copy_from_slice(d);
        Digits { len: d.len(), buf }
    }

    /// The digits as ASCII bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    /// The digits as a string slice.
    pub fn as_str(&self) -> &str {
        // Only ASCII digits are ever stored.
        std::str::from_utf8(self.as_bytes()).unwrap_or("")
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl std::fmt::Debug for Digits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Digits({:?})", self.as_str())
    }
}

impl PartialEq for Digits {
    fn eq(&self, other: &Digits) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl Eq for Digits {}

/// The shortest round-trip digits of `f` and their exponent (`d1.d2... ×
/// 10^x`). Meant for finite nonzero `f`; the sign is ignored and zero gives
/// `("0", 0)`.
pub fn shortest(f: f64) -> (Digits, i32) {
    if f == 0.0 {
        return (Digits::from_slice(b"0"), 0);
    }
    let mut buf = [0u8; 20];
    let (len, x) = shortest_raw(f.abs(), &mut buf);
    (Digits::from_slice(&buf[..len]), x)
}

/// The first `ndigits` significant digits of the exact value of `f`,
/// correctly rounded (exact ties to even), trailing zeros stripped, and
/// their exponent (`d1.d2... × 10^x`). Meant for finite nonzero `f`; the
/// sign is ignored and zero gives `("0", 0)`. `ndigits` of 0 is treated as
/// 1; beyond the exact expansion's length the exact digits are returned.
pub fn fixed_digits(f: f64, ndigits: usize) -> (Digits, i32) {
    if f == 0.0 {
        return (Digits::from_slice(b"0"), 0);
    }
    let a = f.abs();
    let n = ndigits.clamp(1, MAX_DIGITS);
    if n <= 15 && a >= f64::MIN_POSITIVE {
        let mut buf = [0u8; 20];
        let (len, x) = fixed_small(a, n, &mut buf);
        (Digits::from_slice(&buf[..len]), x)
    } else {
        let mut d = Digits {
            len: 0,
            buf: [0u8; MAX_DIGITS],
        };
        let (len, x) = fixed_exact(a, n, &mut d.buf);
        d.len = len;
        (d, x)
    }
}

/// Appends PHP's string form of `f` for ini `precision` = `precision`, as
/// `echo` and `(string)` produce it (`-1`: shortest round-trip; `0` and
/// other values below 1 behave as 1, as observed on PHP 8.5). Observed on
/// PHP 8.5: this conversion truncates "INF", "-INF" and "NAN" to
/// `precision` characters (precision 1 gives "I", "-", "N"; 2 gives "IN",
/// "-I", "NA"), and NAN never shows a sign.
pub fn format_php(f: f64, precision: i32, out: &mut Vec<u8>) {
    if !f.is_finite() {
        let s: &[u8] = if f.is_nan() {
            b"NAN"
        } else if f < 0.0 {
            b"-INF"
        } else {
            b"INF"
        };
        let n = if precision == -1 {
            s.len()
        } else {
            (precision.max(1) as usize).min(s.len())
        };
        out.extend_from_slice(&s[..n]);
        return;
    }
    format_php_repr(f, precision, b'E', out);
}

/// [`format_php`] without the truncation of "INF"/"-INF"/"NAN", and with
/// `exp_marker` as the exponent letter. Observed on PHP 8.5: `var_dump`,
/// `serialize` and `var_export` lay floats out this way with
/// `serialize_precision` (`b'E'`; `var_export` then appends ".0" when the
/// result looks integral), `json_encode` with `b'e'` ("1.0e+17").
pub fn format_php_repr(f: f64, precision: i32, exp_marker: u8, out: &mut Vec<u8>) {
    if f.is_nan() {
        out.extend_from_slice(b"NAN");
        return;
    }
    if f.is_sign_negative() {
        out.push(b'-');
    }
    let a = f.abs();
    if a == f64::INFINITY {
        out.extend_from_slice(b"INF");
        return;
    }
    if a == 0.0 {
        out.push(b'0');
        return;
    }
    let p: usize = if precision == -1 {
        17
    } else if precision < 1 {
        1
    } else {
        precision as usize
    };
    // Integers below 2^53 with at most `p` digits print as themselves.
    if a < 9_007_199_254_740_992.0 && a.trunc() == a && (p > 16 || a < POW10[p]) {
        write_u64(a as u64, out);
        return;
    }
    let mut buf = [0u8; 20];
    if precision == -1 {
        let (len, x) = shortest_raw(a, &mut buf);
        layout(&buf[..len], x, p, exp_marker, out);
    } else if p <= 15 && a >= f64::MIN_POSITIVE {
        let (len, x) = fixed_small(a, p, &mut buf);
        layout(&buf[..len], x, p, exp_marker, out);
    } else {
        format_long(a, p, exp_marker, out);
    }
}

/// [`format_php`] into a new `String`.
pub fn to_php_string(f: f64, precision: i32) -> String {
    let mut v = Vec::with_capacity(24);
    format_php(f, precision, &mut v);
    // format_php only writes ASCII.
    String::from_utf8(v).unwrap_or_default()
}

#[inline(never)]
fn format_long(a: f64, p: usize, exp_marker: u8, out: &mut Vec<u8>) {
    let mut buf = [0u8; MAX_DIGITS];
    let (len, x) = fixed_exact(a, p.min(MAX_DIGITS), &mut buf);
    layout(&buf[..len], x, p, exp_marker, out);
}

const POW10: [f64; 17] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
];

/// PHP's layout of `d1.d2... × 10^x` for precision `p` (observed on PHP
/// 8.5): exponential form when `x < -4` or `x >= p`, with at least one digit
/// after the point and a signed exponent ("1.0E+25", "1.5E-7"); otherwise
/// plain decimal without trailing zeros or point ("0.0001234", "100000").
fn layout(d: &[u8], x: i32, p: usize, exp_marker: u8, out: &mut Vec<u8>) {
    if x < -4 || x as i64 >= p as i64 {
        out.push(d[0]);
        out.push(b'.');
        if d.len() > 1 {
            out.extend_from_slice(&d[1..]);
        } else {
            out.push(b'0');
        }
        out.push(exp_marker);
        out.push(if x < 0 { b'-' } else { b'+' });
        write_u64(x.unsigned_abs() as u64, out);
    } else if x < 0 {
        out.extend_from_slice(b"0.");
        out.resize(out.len() + (-x - 1) as usize, b'0');
        out.extend_from_slice(d);
    } else {
        let ip = x as usize + 1;
        if d.len() <= ip {
            out.extend_from_slice(d);
            out.resize(out.len() + (ip - d.len()), b'0');
        } else {
            out.extend_from_slice(&d[..ip]);
            out.push(b'.');
            out.extend_from_slice(&d[ip..]);
        }
    }
}

fn write_u64(mut v: u64, out: &mut Vec<u8>) {
    let mut tmp = [0u8; 20];
    let mut i = tmp.len();
    loop {
        i -= 1;
        tmp[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    out.extend_from_slice(&tmp[i..]);
}

/// Writes the decimal digits of `v` (nonzero) without trailing zeros into
/// `buf`; returns (digit count, number of trailing zeros removed + digit
/// count - 1), i.e. the exponent of the first digit relative to `v`'s
/// units.
fn emit_u64(mut v: u64, buf: &mut [u8; 20]) -> (usize, i32) {
    let mut zeros = 0;
    while v.is_multiple_of(10) {
        v /= 10;
        zeros += 1;
    }
    let mut tmp = [0u8; 20];
    let mut i = tmp.len();
    while v != 0 {
        i -= 1;
        tmp[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    let len = tmp.len() - i;
    buf[..len].copy_from_slice(&tmp[i..]);
    (len, zeros + len as i32 - 1)
}

// ---------------------------------------------------------------- logs

/// floor(q · log10 2), exact for |q| <= 1100 (checked by a test against the
/// bignum). The constant is floor(log10(2) · 2^32).
fn flog10pow2(q: i32) -> i32 {
    ((q as i64 * 1_292_913_986) >> 32) as i32
}

/// floor(q · log10 2 + log10(3/4)), exact for |q| <= 1100 (tested).
fn flog10_3q_pow2(q: i32) -> i32 {
    ((q as i64 * 1_292_913_986 - 536_607_788) >> 32) as i32
}

/// floor(e · log2 10), exact for |e| <= 350 (tested). The constant is
/// floor(log2(10) · 2^32).
fn flog2pow10(e: i32) -> i32 {
    ((e as i64 * 14_267_572_527) >> 32) as i32
}

// ---------------------------------------------------------------- bignum

const LIMBS: usize = 40;

/// Unsigned integer of up to 1280 bits, little-endian 32-bit limbs; `len`
/// counts the used limbs (the top one is nonzero, zero has `len == 0`).
#[derive(Clone, Copy)]
struct Big {
    len: usize,
    d: [u32; LIMBS],
}

impl Big {
    fn from_u64(v: u64) -> Big {
        let mut b = Big {
            len: 2,
            d: [0; LIMBS],
        };
        b.d[0] = v as u32;
        b.d[1] = (v >> 32) as u32;
        b.trim();
        b
    }

    #[cfg(test)]
    fn from_u128(v: u128) -> Big {
        let mut b = Big {
            len: 4,
            d: [0; LIMBS],
        };
        for i in 0..4 {
            b.d[i] = (v >> (32 * i)) as u32;
        }
        b.trim();
        b
    }

    fn trim(&mut self) {
        while self.len > 0 && self.d[self.len - 1] == 0 {
            self.len -= 1;
        }
    }

    fn is_zero(&self) -> bool {
        self.len == 0
    }

    fn mul_small(&mut self, m: u32) {
        let mut carry = 0u64;
        for i in 0..self.len {
            let t = self.d[i] as u64 * m as u64 + carry;
            self.d[i] = t as u32;
            carry = t >> 32;
        }
        if carry != 0 {
            self.d[self.len] = carry as u32;
            self.len += 1;
        }
    }

    fn mul_pow10(&mut self, mut n: u32) {
        while n >= 9 {
            self.mul_small(1_000_000_000);
            n -= 9;
        }
        if n > 0 {
            self.mul_small(10u32.pow(n));
        }
    }

    fn shl(&mut self, bits: u32) {
        if self.len == 0 || bits == 0 {
            return;
        }
        let limbs = (bits / 32) as usize;
        let b = bits % 32;
        if b == 0 {
            for i in (0..self.len).rev() {
                self.d[i + limbs] = self.d[i];
            }
        } else {
            let top = self.d[self.len - 1] >> (32 - b);
            self.d[self.len + limbs] = top;
            for i in (1..self.len).rev() {
                self.d[i + limbs] = (self.d[i] << b) | (self.d[i - 1] >> (32 - b));
            }
            self.d[limbs] = self.d[0] << b;
        }
        for i in 0..limbs {
            self.d[i] = 0;
        }
        self.len += limbs + (b != 0) as usize;
        self.trim();
    }

    fn cmp(&self, o: &Big) -> Ordering {
        if self.len != o.len {
            return self.len.cmp(&o.len);
        }
        for i in (0..self.len).rev() {
            if self.d[i] != o.d[i] {
                return self.d[i].cmp(&o.d[i]);
            }
        }
        Ordering::Equal
    }

    /// self -= o; requires self >= o.
    fn sub_assign(&mut self, o: &Big) {
        let mut borrow = 0i64;
        for i in 0..self.len {
            let t = self.d[i] as i64 - borrow - if i < o.len { o.d[i] as i64 } else { 0 };
            if t < 0 {
                self.d[i] = (t + (1i64 << 32)) as u32;
                borrow = 1;
            } else {
                self.d[i] = t as u32;
                borrow = 0;
            }
        }
        self.trim();
    }

    /// self -= q · o; requires self >= q · o.
    fn sub_mul_small(&mut self, o: &Big, q: u32) {
        let mut carry = 0u64;
        let mut borrow = 0i64;
        for i in 0..self.len {
            let p = if i < o.len {
                o.d[i] as u64 * q as u64 + carry
            } else {
                carry
            };
            carry = p >> 32;
            let t = self.d[i] as i64 - borrow - (p as u32) as i64;
            if t < 0 {
                self.d[i] = (t + (1i64 << 32)) as u32;
                borrow = 1;
            } else {
                self.d[i] = t as u32;
                borrow = 0;
            }
        }
        self.trim();
    }

    /// self /= v, returning the remainder.
    fn div_small(&mut self, v: u32) -> u32 {
        let mut rem = 0u64;
        for i in (0..self.len).rev() {
            let t = (rem << 32) | self.d[i] as u64;
            self.d[i] = (t / v as u64) as u32;
            rem = t % v as u64;
        }
        self.trim();
        rem as u32
    }

    fn limb(&self, i: usize) -> u32 {
        if i < self.len { self.d[i] } else { 0 }
    }

    /// floor(self / 2^bits), which must fit in 128 bits.
    fn shr_to_u128(&self, bits: u32) -> u128 {
        let mut r = 0u128;
        for j in 0..4 {
            let pos = bits as usize + 32 * j;
            let (i, b) = (pos / 32, pos % 32);
            let w = if b == 0 {
                self.limb(i)
            } else {
                (self.limb(i) >> b) | (self.limb(i + 1) << (32 - b))
            };
            r |= (w as u128) << (32 * j);
        }
        r
    }

    /// For self < 10 · s with `s` normalized (top limb's high bit set):
    /// returns floor(self / s) and leaves the remainder in self.
    fn div_digit(&mut self, s: &Big) -> u8 {
        let n = s.len;
        let top = if self.len > n {
            ((self.d[n] as u64) << 32) | self.d[n - 1] as u64
        } else if self.len == n {
            self.d[n - 1] as u64
        } else {
            return 0;
        };
        // An underestimate by at most one.
        let mut q = (top / (s.d[n - 1] as u64 + 1)) as u32;
        if q > 0 {
            self.sub_mul_small(s, q);
        }
        while self.cmp(s) != Ordering::Less {
            self.sub_assign(s);
            q += 1;
        }
        q as u8
    }
}

// ---------------------------------------------------------------- exact digits

/// f = m · 2^e with m > 0 (for finite nonzero f).
fn decompose(f: f64) -> (u64, i32) {
    let bits = f.to_bits();
    let be = ((bits >> 52) & 0x7ff) as i32;
    let fr = bits & ((1u64 << 52) - 1);
    if be == 0 {
        (fr, -1074)
    } else {
        (fr | (1u64 << 52), be - 1075)
    }
}

/// Rounds the `len` digits in `buf` up by one unit in the last place,
/// carrying; an all-nines string becomes "1" with the exponent bumped.
fn round_up(buf: &mut [u8], len: &mut usize, x: &mut i32) {
    let mut i = *len;
    loop {
        if i == 0 {
            buf[0] = b'1';
            *len = 1;
            *x += 1;
            return;
        }
        i -= 1;
        if buf[i] == b'9' {
            buf[i] = b'0';
        } else {
            buf[i] += 1;
            *len = i + 1;
            return;
        }
    }
}

fn strip_zeros(buf: &[u8], mut len: usize) -> usize {
    while len > 1 && buf[len - 1] == b'0' {
        len -= 1;
    }
    len
}

/// The first `n` (1 ≤ n ≤ buf.len()) significant digits of finite f > 0,
/// correctly rounded half to even, by exact bignum digit generation.
fn fixed_exact(f: f64, n: usize, buf: &mut [u8]) -> (usize, i32) {
    let (m, e) = decompose(f);
    let n = n.clamp(1, buf.len());
    let bl = 64 - m.leading_zeros() as i32;
    // 2^(e+bl-1) <= f < 2^(e+bl), so 10^k <= f < 10^(k+2).
    let mut k = flog10pow2(e + bl - 1);
    let mut r = Big::from_u64(m);
    let mut s = Big::from_u64(1);
    if e >= 0 {
        r.shl(e as u32);
    } else {
        s.shl((-e) as u32);
    }
    if k >= 0 {
        s.mul_pow10(k as u32);
    } else {
        r.mul_pow10((-k) as u32);
    }
    let mut s10 = s;
    s10.mul_small(10);
    if r.cmp(&s10) != Ordering::Less {
        s = s10;
        k += 1;
    }
    // Now 1 <= r/s < 10. Normalize s for the quotient estimate.
    let sh = s.d[s.len - 1].leading_zeros();
    r.shl(sh);
    s.shl(sh);
    let mut len = 0;
    loop {
        buf[len] = b'0' + r.div_digit(&s);
        len += 1;
        if r.is_zero() || len == n {
            break;
        }
        r.mul_small(10);
    }
    if !r.is_zero() {
        r.shl(1);
        let up = match r.cmp(&s) {
            Ordering::Greater => true,
            Ordering::Equal => (buf[len - 1] - b'0') & 1 == 1,
            Ordering::Less => false,
        };
        if up {
            round_up(buf, &mut len, &mut k);
        }
    }
    (strip_zeros(buf, len), k)
}

/// `fixed_exact` for normal f > 0 and 1 <= n <= 15, through the shortest
/// digits S when that gives the same answer. For a normal double the
/// spacing of n-digit decimals (n <= 15) exceeds the width of its rounding
/// interval, so: if S has at most n digits it is the nearest n-digit
/// decimal; if it has more than n + 1 digits no n-digit rounding midpoint
/// (an (n+1)-digit decimal) lies in the interval, so rounding S and rounding
/// f agree; with exactly n + 1 digits the same holds unless S itself ends
/// in 5 (it may then be the midpoint), which goes to the exact path.
fn fixed_small(f: f64, n: usize, buf: &mut [u8; 20]) -> (usize, i32) {
    let (len, mut x) = shortest_raw(f, buf);
    if len <= n {
        return (len, x);
    }
    if len == n + 1 && buf[n] == b'5' {
        return fixed_exact(f, n, &mut buf[..]);
    }
    let mut len = n;
    if buf[n] >= b'5' {
        round_up(buf, &mut len, &mut x);
    }
    (strip_zeros(buf, len), x)
}

// ---------------------------------------------------------------- Schubfach

const G_MIN: i32 = -292;
const G_MAX: i32 = 324;
const G_LEN: usize = (G_MAX - G_MIN + 1) as usize;

/// g(e) = floor(10^e · 2^(125 - floor(e·log2 10))) + 1 for e in
/// [G_MIN, G_MAX], so 2^125 < g(e) <= 2^126: a 126-bit over-approximation
/// of 10^e (Giulietti 2020). Computed from the definition: positive powers
/// as exact 5^e shifted, negative ones from floor(2^1000 / 5^j), using
/// floor(floor(x)/n) = floor(x/n) for integer n.
fn g_table() -> &'static [u128; G_LEN] {
    static T: OnceLock<[u128; G_LEN]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u128; G_LEN];
        let mut p5 = Big::from_u64(1);
        for e in 0..=G_MAX {
            if e > 0 {
                p5.mul_small(5);
            }
            let sh = e + 125 - flog2pow10(e);
            let v = if sh >= 0 {
                let mut b = p5;
                b.shl(sh as u32);
                b.shr_to_u128(0)
            } else {
                p5.shr_to_u128((-sh) as u32)
            };
            t[(e - G_MIN) as usize] = v + 1;
        }
        const B: i32 = 1000;
        let mut x = Big::from_u64(1);
        x.shl(B as u32);
        for j in 1..=-G_MIN {
            x.div_small(5);
            let e = -j;
            // 10^e · 2^(125 - flog2pow10(e)) = 2^a / 5^j.
            let a = 125 - flog2pow10(e) - j;
            t[(e - G_MIN) as usize] = x.shr_to_u128((B - a) as u32) + 1;
        }
        t
    })
}

/// Giulietti's round-to-odd product: with g split into 63-bit halves
/// g1·2^63 + g0, approximately (g · cp) / 2^127, computed from the high
/// 64-bit words of g1·cp and g0·cp only, with the low bit set when the
/// discarded part of that sum is nonzero. Ignoring the low word of g0·cp is
/// what makes the result exact when 10^-k is an exact integer (g is then
/// one above it).
#[inline]
fn rop(g: u128, cp: u64) -> u64 {
    const MASK63: u64 = (1u64 << 63) - 1;
    let g1 = (g >> 63) as u64;
    let g0 = g as u64 & MASK63;
    let x1 = ((g0 as u128 * cp as u128) >> 64) as u64;
    let y = g1 as u128 * cp as u128;
    let z = ((y as u64) >> 1) + x1;
    let vbp = (y >> 64) as u64 + (z >> 63);
    vbp | (z & MASK63 != 0) as u64
}

/// Shortest round-trip digits of finite f > 0 into `buf`: (len, exponent).
fn shortest_raw(f: f64, buf: &mut [u8; 20]) -> (usize, i32) {
    let bits = f.to_bits();
    let be = ((bits >> 52) & 0x7ff) as i32;
    let fr = bits & ((1u64 << 52) - 1);
    let (c, q) = if be == 0 {
        (fr, -1074)
    } else {
        (fr | (1u64 << 52), be - 1075)
    };
    // Integers below 2^53: the spacing is at most 1, so no other decimal
    // with as few significant digits rounds to f.
    if (-52..=0).contains(&q) && c.trailing_zeros() as i32 >= -q {
        let (len, x) = emit_u64(c >> -q, buf);
        return (len, x);
    }
    // Rounding interval in units of 2^(q-2): [cbl, cbr] around cb = 4c,
    // closed when c is even (round-half-even parsing). Its lower half is
    // halved when f is a power of two above the subnormal range.
    let cb = c << 2;
    let cbr = cb + 2;
    let (cbl, k) = if fr != 0 || be <= 1 {
        (cb - 2, flog10pow2(q))
    } else {
        (cb - 1, flog10_3q_pow2(q))
    };
    let g = g_table()[(-k - G_MIN) as usize];
    let h = q + flog2pow10(-k) + 2;
    // vb ≈ 4 · f · 10^-k, and likewise for the bounds; the interval is
    // between 1 and 10 units of 10^k wide.
    let vb = rop(g, cb << h);
    let vbl = rop(g, cbl << h);
    let vbr = rop(g, cbr << h);
    let out = c & 1;
    let s = vb >> 2;
    // One digit fewer: at most one multiple of 10 fits in the interval.
    let sp10 = s / 10 * 10;
    let tp10 = sp10 + 10;
    let upin = vbl + out <= sp10 << 2;
    let wpin = (tp10 << 2) + out <= vbr;
    let d = if upin != wpin {
        if upin { sp10 } else { tp10 }
    } else {
        // s or s + 1 is in the interval; take the one inside, or the
        // closer one (ties to even) when both are.
        let t = s + 1;
        let uin = vbl + out <= s << 2;
        let win = (t << 2) + out <= vbr;
        if uin != win {
            if uin { s } else { t }
        } else {
            let mid = (s + t) << 1;
            if vb < mid || (vb == mid && s & 1 == 0) {
                s
            } else {
                t
            }
        }
    };
    let (len, x) = emit_u64(d, buf);
    (len, x + k)
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            // xorshift64*
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
        fn finite(&mut self) -> f64 {
            loop {
                let f = f64::from_bits(self.next() & !(1u64 << 63));
                if f.is_finite() && f != 0.0 {
                    return f;
                }
            }
        }
    }

    fn pstr(f: f64, p: i32) -> String {
        to_php_string(f, p)
    }

    /// Rust's own `{:e}` (shortest, closest) as (digits, exponent).
    fn rust_shortest(f: f64) -> (String, i32) {
        let s = format!("{:e}", f);
        let (m, e) = s.split_once('e').unwrap();
        (m.replace('.', ""), e.parse().unwrap())
    }

    #[test]
    fn fixture() {
        let text = include_str!("../tests/fixtures/php85_fixtures.txt");
        let mut in_section = false;
        let mut rows = 0;
        let mut failures = Vec::new();
        for line in text.lines() {
            if line.starts_with('@') {
                in_section = line == "@floats";
                continue;
            }
            if !in_section {
                continue;
            }
            let cols: Vec<&str> = line.split('\t').collect();
            assert_eq!(cols.len(), 7, "bad row {line:?}");
            let f = f64::from_bits(u64::from_str_radix(cols[0], 16).unwrap());
            for (i, p) in [14, 17, -1, 1, 5, 20].into_iter().enumerate() {
                let got = pstr(f, p);
                if got != cols[i + 1] {
                    failures.push(format!(
                        "{} p={p}: got {got}, want {}",
                        cols[0],
                        cols[i + 1]
                    ));
                }
            }
            rows += 1;
        }
        assert!(rows > 1000, "only {rows} fixture rows");
        assert!(
            failures.is_empty(),
            "{} failures:\n{}",
            failures.len(),
            failures[..failures.len().min(30)].join("\n")
        );
    }

    /// c · 10^p10 · 2^p2 for p10, p2 >= 0.
    fn mk(c: u64, p10: i32, p2: i32) -> Big {
        let mut b = Big::from_u64(c);
        b.mul_pow10(p10 as u32);
        b.shl(p2 as u32);
        b
    }

    /// a <= b < 10·a (or 2·a).
    fn in_decade(a: &Big, b: &Big, base: u32) -> bool {
        let mut a10 = *a;
        a10.mul_small(base);
        a.cmp(b) != Ordering::Greater && b.cmp(&a10) == Ordering::Less
    }

    #[test]
    fn log_approximations() {
        for q in -1100..=1100 {
            // 10^k <= 2^q < 10^(k+1)
            let k = flog10pow2(q);
            let a = mk(1, k.max(0), (-q).max(0));
            let b = mk(1, (-k).max(0), q.max(0));
            assert!(in_decade(&a, &b, 10), "q={q}");
            // 10^k <= 3/4 · 2^q < 10^(k+1)
            let k = flog10_3q_pow2(q);
            let a = mk(4, k.max(0), (-q).max(0));
            let b = mk(3, (-k).max(0), q.max(0));
            assert!(in_decade(&a, &b, 10), "q={q} (3/4)");
        }
        for e in -350..=350 {
            // 2^b <= 10^e < 2^(b+1)
            let b = flog2pow10(e);
            let lo = mk(1, (-e).max(0), b.max(0));
            let v = mk(1, e.max(0), (-b).max(0));
            assert!(in_decade(&lo, &v, 2), "e={e}");
        }
    }

    #[test]
    fn g_table_definition() {
        let t = g_table();
        for e in G_MIN..=G_MAX {
            let g = t[(e - G_MIN) as usize];
            assert!(g > 1u128 << 125 && g <= 1u128 << 126, "e={e}");
            // (g-1) <= 10^e · 2^sh < g
            let sh = 125 - flog2pow10(e);
            let mut lo = Big::from_u128(g - 1);
            let mut hi = Big::from_u128(g);
            let mut v = Big::from_u64(1);
            if e >= 0 {
                v.mul_pow10(e as u32);
            } else {
                lo.mul_pow10((-e) as u32);
                hi.mul_pow10((-e) as u32);
            }
            if sh >= 0 {
                v.shl(sh as u32);
            } else {
                lo.shl((-sh) as u32);
                hi.shl((-sh) as u32);
            }
            assert_ne!(lo.cmp(&v), Ordering::Greater, "e={e}");
            assert_eq!(v.cmp(&hi), Ordering::Less, "e={e}");
        }
    }

    fn check_shortest(f: f64) {
        let (d, x) = shortest(f);
        let (rd, rx) = rust_shortest(f);
        if (d.as_str(), x) != (rd.as_str(), rx) {
            // Rust's formatter breaks exact ties between two closest
            // candidates differently; PHP 8.5 takes the even one (observed:
            // 2^-25 gives 2.9802322387695312E-8). Accept only such ties.
            let (exact, ex) = fixed_digits(f, MAX_DIGITS);
            let tie = d.len() == rd.len()
                && x == rx
                && exact.len() == d.len() + 1
                && ex == x
                && exact.as_bytes()[d.len()] == b'5'
                && (d.as_bytes()[d.len() - 1] - b'0').is_multiple_of(2);
            assert!(tie, "bits {:016x}: {d:?} {x} vs {rd} {rx}", f.to_bits());
        }
        let back: f64 = format!("{}e{}", d.as_str(), x - d.len() as i32 + 1)
            .parse()
            .unwrap();
        assert_eq!(back.to_bits(), f.to_bits());
    }

    #[test]
    fn shortest_special_values() {
        for be in 0u64..0x7ff {
            for fr in [0u64, 1, 2, 3, (1 << 52) - 1, (1 << 52) - 2, 1 << 51] {
                let f = f64::from_bits((be << 52) | fr);
                if f != 0.0 {
                    check_shortest(f);
                }
            }
        }
        for v in [
            1.0,
            0.1,
            0.3,
            1e23,
            f64::from_bits(0x44b5_2d02_c7e1_4af5), // just below 1e23
            5e-324,
            f64::MAX,
            f64::MIN_POSITIVE,
            2.225073858507201e-308,
            123.456,
            9007199254740993.0,
            1.7976931348623157e308,
        ] {
            check_shortest(v);
        }
        for i in 1..=10_000u64 {
            check_shortest(i as f64);
            check_shortest(i as f64 / 1000.0);
            check_shortest(f64::from_bits(i));
        }
    }

    #[test]
    fn shortest_random_against_rust() {
        let mut rng = Rng(0x1234_5678_9abc_def1);
        for _ in 0..300_000 {
            check_shortest(rng.finite());
        }
    }

    fn rust_fixed(f: f64, n: usize) -> (String, i32) {
        let s = format!("{:.*e}", n - 1, f);
        let (m, e) = s.split_once('e').unwrap();
        let mut m = m.replace('.', "");
        while m.len() > 1 && m.ends_with('0') {
            m.pop();
        }
        (m, e.parse().unwrap())
    }

    #[test]
    fn fixed_random_against_exact_and_rust() {
        let mut rng = Rng(0x0fed_cba9_8765_4321);
        for i in 0..100_000 {
            let f = rng.finite();
            let n = 1 + (i % 40);
            let (d, x) = fixed_digits(f, n);
            let mut buf = [0u8; MAX_DIGITS];
            let (len, ex) = fixed_exact(f, n, &mut buf);
            assert_eq!(
                (d.as_bytes(), x),
                (&buf[..len], ex),
                "bits {:016x} n={n}",
                f.to_bits()
            );
            let (rd, rx) = rust_fixed(f, n);
            assert_eq!(
                (d.as_str(), x),
                (rd.as_str(), rx),
                "bits {:016x} n={n}",
                f.to_bits()
            );
        }
    }

    #[test]
    fn fixed_short_decimals() {
        // Values near short decimals stress the fast path's midpoint test.
        let mut rng = Rng(42);
        for _ in 0..100_000 {
            let digits = rng.next() % 1_000_000_000_000_000;
            let exp = (rng.next() % 60) as i32 - 30;
            let f: f64 = format!("{digits}e{exp}").parse().unwrap();
            if f == 0.0 {
                continue;
            }
            for n in [1, 2, 5, 13, 14, 15] {
                let (d, x) = fixed_digits(f, n);
                let mut buf = [0u8; MAX_DIGITS];
                let (len, ex) = fixed_exact(f, n, &mut buf);
                assert_eq!((d.as_bytes(), x), (&buf[..len], ex), "{f:e} n={n}");
            }
        }
    }

    #[test]
    fn fixed_ties_and_carries() {
        let fx = |f: f64, n: usize| {
            let (d, x) = fixed_digits(f, n);
            (d.as_str().to_string(), x)
        };
        assert_eq!(fx(1.5, 1), ("2".into(), 0));
        assert_eq!(fx(2.5, 1), ("2".into(), 0));
        assert_eq!(fx(3.5, 1), ("4".into(), 0));
        assert_eq!(fx(0.125, 1), ("1".into(), -1));
        assert_eq!(fx(0.375, 1), ("4".into(), -1));
        assert_eq!(fx(0.125, 2), ("12".into(), -1));
        assert_eq!(fx(9.99, 2), ("1".into(), 1));
        assert_eq!(
            fx(f64::from_bits(0x44b5_2d02_c7e1_4af5), 14),
            ("1".into(), 23)
        );
        assert_eq!(fx(0.1, 20), ("10000000000000000555".into(), -1));
        assert_eq!(
            fx(0.1, 100),
            (
                "1000000000000000055511151231257827021181583404541015625".into(),
                -1
            )
        );
        let (d, x) = fixed_digits(5e-324, 1000);
        assert_eq!(x, -324);
        assert_eq!(d.len(), 751);
        assert!(
            d.as_str()
                .starts_with("49406564584124654417656879286822137236505980")
        );
        assert!(d.as_str().ends_with("06419718265533447265625"));
        // The longest exact expansion fits.
        let (d, _) = fixed_digits(f64::from_bits((1 << 52) - 1), 5000);
        assert!(d.len() <= MAX_DIGITS);
        assert_eq!(d.len(), 767);
    }

    #[test]
    fn php_layout() {
        let cases: &[(f64, i32, &str)] = &[
            (0.0, 14, "0"),
            (-0.0, 14, "-0"),
            (-0.0, -1, "-0"),
            (f64::NAN, 14, "NAN"),
            (f64::INFINITY, 14, "INF"),
            (f64::NEG_INFINITY, -1, "-INF"),
            (1e25, 14, "1.0E+25"),
            (1234567890123456.0, 14, "1.2345678901235E+15"),
            (1e-5, 14, "1.0E-5"),
            (5e-324, 14, "4.9406564584125E-324"),
            (-100.0, 1, "-1.0E+2"),
            (0.0001234, 14, "0.0001234"),
            (123456789012345.0, 15, "123456789012345"),
            (123456789012345.0, 14, "1.2345678901234E+14"),
            (0.5, 14, "0.5"),
            (100000.0, 14, "100000"),
            (123456.0, 0, "1.0E+5"),
            (2.5, 0, "2"),
            (95.0, 0, "1.0E+2"),
            (0.1, 17, "0.10000000000000001"),
            (0.1, 20, "0.10000000000000000555"),
            (0.1, -1, "0.1"),
            (1e15, -1, "1000000000000000"),
            (1e16, -1, "10000000000000000"),
            (f64::INFINITY, 1, "I"),
            (f64::INFINITY, 0, "I"),
            (f64::INFINITY, 2, "IN"),
            (f64::NEG_INFINITY, 1, "-"),
            (f64::NEG_INFINITY, 3, "-IN"),
            (f64::NEG_INFINITY, 4, "-INF"),
            (f64::NAN, 2, "NA"),
            (-f64::NAN, 14, "NAN"),
            (2.9802322387695312e-8, -1, "2.9802322387695312E-8"),
            (1341538170625153.2, -1, "1341538170625153.2"),
            (5.551115123125783e-17, -1, "5.551115123125783E-17"),
            (5.551115123125783e-17, 17, "5.5511151231257827E-17"),
            (1e22, 1000, "10000000000000000000000"),
            (-1.5, -1, "-1.5"),
            (0.30000000000000004, -1, "0.30000000000000004"),
            (1e17, 20, "100000000000000000"),
            (1e17, -1, "1.0E+17"),
            (9007199254740993.0, -1, "9007199254740992"),
            (f64::MAX, -1, "1.7976931348623157E+308"),
        ];
        for &(f, p, want) in cases {
            assert_eq!(pstr(f, p), want, "{f:e} p={p}");
        }
    }

    /// Long randomized cross-check: `cargo test --release -- --ignored stress`.
    #[test]
    #[ignore]
    fn stress() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        for i in 0..20_000_000u64 {
            let f = rng.finite();
            check_shortest(f);
            if i % 8 == 0 {
                let n = 1 + (i / 8 % 17) as usize;
                let (d, x) = fixed_digits(f, n);
                let mut buf = [0u8; MAX_DIGITS];
                let (len, ex) = fixed_exact(f, n, &mut buf);
                assert_eq!(
                    (d.as_bytes(), x),
                    (&buf[..len], ex),
                    "{:016x} n={n}",
                    f.to_bits()
                );
            }
        }
    }

    /// Rough timings: `cargo test --release -- --ignored --nocapture timing`.
    #[test]
    #[ignore]
    fn timing() {
        use std::time::Instant;
        let mut rng = Rng(7);
        let randoms: Vec<f64> = (0..10_000).map(|_| rng.finite()).collect();
        let sets: [(&str, Vec<f64>); 5] = [
            ("0.1", vec![0.1; 10_000]),
            ("123.456", vec![123.456; 10_000]),
            ("1e100", vec![1e100; 10_000]),
            ("42.0", vec![42.0; 10_000]),
            ("random", randoms),
        ];
        let mut out = Vec::with_capacity(64);
        for (name, vals) in &sets {
            for p in [-1, 14, 17] {
                let start = Instant::now();
                let rounds = 20;
                for _ in 0..rounds {
                    for &v in vals {
                        out.clear();
                        format_php(std::hint::black_box(v), p, &mut out);
                        std::hint::black_box(&out);
                    }
                }
                let ns = start.elapsed().as_nanos() as f64 / (rounds * vals.len()) as f64;
                println!("{name:>8} p={p:>3}: {ns:.1} ns");
            }
        }
    }
}
