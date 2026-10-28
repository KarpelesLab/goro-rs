//! Variable-time edwards25519 arithmetic for signature verification and
//! public-key checks: point decoding (RFC 8032 §5.1.3), small-order tests
//! and `[a]P + [b]B` by interleaved w-NAF (Hankerson, Menezes, Vanstone,
//! "Guide to Elliptic Curve Cryptography", Alg. 3.35 and 3.51).
//!
//! Only public data (keys, signatures, hashes of public data) flows
//! through here; secret-scalar operations use purecrypto's constant-time
//! code. Point formulas: Hisil, Wong, Carter, Dawson, "Twisted Edwards
//! Curves Revisited" (extended coordinates, a = −1), with the "completed"
//! intermediate representation of §3 of that paper.

use crate::field::Fe;
use std::sync::OnceLock;

struct Consts {
    d: Fe,
    d2: Fe,
    sqrt_m1: Fe,
    /// Odd multiples B, 3B, …, 127B of the base point in affine Niels form.
    base: Vec<Niels>,
    /// Every 32-byte string that decodes to a point of small order.
    small_order: Vec<[u8; 32]>,
}

/// A point of order 8 (checked by the tests).
const T8: [u8; 32] = [
    0xc7, 0x17, 0x6a, 0x70, 0x3d, 0x4d, 0xd8, 0x4f, 0xba, 0x3c, 0x0b, 0x76, 0x0d, 0x10, 0x67, 0x0f,
    0x2a, 0x20, 0x53, 0xfa, 0x2c, 0x39, 0xcc, 0xc6, 0x4e, 0xc7, 0xfd, 0x77, 0x92, 0xac, 0x03, 0x7a,
];

/// Whether the bytes encode a point of small order, canonically or not.
pub fn is_small_order_encoding(b: &[u8; 32]) -> bool {
    consts().small_order.iter().any(|e| e == b)
}

/// `(y + x, y − x, 2d·x·y)` of an affine point.
#[derive(Clone, Copy)]
struct Niels {
    ypx: Fe,
    ymx: Fe,
    xy2d: Fe,
}

/// `(Y + X, Y − X, Z, 2d·T)` of an extended point.
#[derive(Clone, Copy)]
struct Cached {
    ypx: Fe,
    ymx: Fe,
    z: Fe,
    t2d: Fe,
}

/// Extended coordinates: x = X/Z, y = Y/Z, xy = T/Z.
#[derive(Clone, Copy)]
pub struct P3 {
    x: Fe,
    y: Fe,
    z: Fe,
    t: Fe,
}

/// Completed coordinates: x = X/Z, y = Y/T.
#[derive(Clone, Copy)]
struct P1 {
    x: Fe,
    y: Fe,
    z: Fe,
    t: Fe,
}

/// Projective coordinates: x = X/Z, y = Y/Z.
#[derive(Clone, Copy)]
struct P2 {
    x: Fe,
    y: Fe,
    z: Fe,
}

fn consts() -> &'static Consts {
    static C: OnceLock<Consts> = OnceLock::new();
    C.get_or_init(|| {
        let d = Fe::from_bytes(&[
            0xa3, 0x78, 0x59, 0x13, 0xca, 0x4d, 0xeb, 0x75, 0xab, 0xd8, 0x41, 0x41, 0x4d, 0x0a,
            0x70, 0x00, 0x98, 0xe8, 0x79, 0x77, 0x79, 0x40, 0xc7, 0x8c, 0x73, 0xfe, 0x6f, 0x2b,
            0xee, 0x6c, 0x03, 0x52,
        ]);
        let sqrt_m1 = Fe::from_bytes(&[
            0xb0, 0xa0, 0x0e, 0x4a, 0x27, 0x1b, 0xee, 0xc4, 0x78, 0xe4, 0x2f, 0xad, 0x06, 0x18,
            0x43, 0x2f, 0xa7, 0xd7, 0xfb, 0x3d, 0x99, 0x00, 0x4d, 0x2b, 0x0b, 0xdf, 0xc1, 0x4f,
            0x80, 0x24, 0x83, 0x2b,
        ]);
        let mut c = Consts {
            d,
            d2: d.add(d),
            sqrt_m1,
            base: Vec::new(),
            small_order: Vec::new(),
        };
        // RFC 8032 §5.1: B has y = 4/5 and positive x.
        let mut by = [0x66u8; 32];
        by[0] = 0x58;
        let b = decompress_with(&c, &by, true).expect("base point");
        let b2 = to_p3(dbl(&to_p2(&b)));
        let b2c = cached(&c, &b2);
        let mut cur = b;
        for _ in 0..64 {
            let zi = cur.z.invert();
            let x = cur.x.mul(zi);
            let y = cur.y.mul(zi);
            c.base.push(Niels {
                ypx: y.add(x),
                ymx: y.sub(x),
                xy2d: x.mul(y).mul(c.d2),
            });
            cur = to_p3(add(&cur, &b2c));
        }
        // The eight points of order dividing 8 are the multiples of one of
        // order 8; their encodings, plus the sign-bit variants of the two
        // with x = 0 and the non-canonical y = p, p + 1 encodings.
        let t8 = decompress_with(&c, &T8, false).expect("order-8 point");
        let t8c = cached(&c, &t8);
        let mut cur = P3 {
            x: Fe::ZERO,
            y: Fe::ONE,
            z: Fe::ONE,
            t: Fe::ZERO,
        };
        for _ in 0..8 {
            let e = cur.compress();
            c.small_order.push(e);
            if cur.x.mul(cur.z.invert()).is_zero() {
                let mut neg = e;
                neg[31] |= 0x80;
                c.small_order.push(neg);
            }
            cur = to_p3(add(&cur, &t8c));
        }
        for y_minus_p in [0u8, 1] {
            let mut e = [0xffu8; 32];
            e[0] = 0xed + y_minus_p;
            e[31] = 0x7f;
            c.small_order.push(e);
            e[31] = 0xff;
            c.small_order.push(e);
        }
        c
    })
}

#[inline(always)]
fn decompress_with(c: &Consts, b: &[u8; 32], canonical: bool) -> Option<P3> {
    let sign = b[31] >> 7;
    let y = Fe::from_bytes(b);
    if canonical {
        let mut yb = *b;
        yb[31] &= 0x7f;
        if y.to_bytes() != yb {
            return None;
        }
    }
    let yy = y.square();
    let u = yy.sub(Fe::ONE);
    let v = yy.mul(c.d).add(Fe::ONE);
    let v3 = v.square().mul(v);
    let v7 = v3.square().mul(v);
    let mut x = u.mul(v3).mul(u.mul(v7).pow_p58());
    let vxx = v.mul(x.square());
    if !vxx.sub(u).is_zero() {
        if !vxx.add(u).is_zero() {
            return None;
        }
        x = x.mul(c.sqrt_m1);
    }
    let xb = x.to_bytes();
    if canonical && sign == 1 && xb == [0u8; 32] {
        return None;
    }
    if (xb[0] & 1) != sign {
        x = x.neg();
    }
    Some(P3 {
        x,
        y,
        z: Fe::ONE,
        t: x.mul(y),
    })
}

/// Decodes a point (RFC 8032 §5.1.3); `canonical` rejects `y ≥ p` and the
/// negative-zero `x` encodings.
pub fn decompress(b: &[u8; 32], canonical: bool) -> Option<P3> {
    #[cfg(target_arch = "x86_64")]
    if crate::field::has_bmi2() {
        // SAFETY: BMI2 is available.
        return unsafe { decompress_bmi2(consts(), b, canonical) };
    }
    decompress_with(consts(), b, canonical)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "bmi2")]
unsafe fn decompress_bmi2(c: &Consts, b: &[u8; 32], canonical: bool) -> Option<P3> {
    decompress_with(c, b, canonical)
}

#[inline(always)]
fn to_p2(p: &P3) -> P2 {
    P2 {
        x: p.x,
        y: p.y,
        z: p.z,
    }
}

#[inline(always)]
fn p1_to_p2(p: &P1) -> P2 {
    P2 {
        x: p.x.mul(p.t),
        y: p.y.mul(p.z),
        z: p.z.mul(p.t),
    }
}

#[inline(always)]
fn to_p3(p: P1) -> P3 {
    P3 {
        x: p.x.mul(p.t),
        y: p.y.mul(p.z),
        z: p.z.mul(p.t),
        t: p.x.mul(p.y),
    }
}

/// Doubling: x' = 2xy/(y² − x²), y' = (y² + x²)/(2 − y² + x²).
#[inline(always)]
fn dbl(p: &P2) -> P1 {
    let xx = p.x.square();
    let yy = p.y.square();
    let zz = p.z.square();
    let zz2 = zz.add(zz);
    let xy2 = p.x.add(p.y).square();
    let yy_plus_xx = yy.add(xx);
    let yy_minus_xx = yy.sub_lazy(xx);
    P1 {
        x: xy2.sub_lazy4(yy_plus_xx),
        y: yy_plus_xx,
        z: yy_minus_xx,
        t: zz2.sub_lazy4(yy_minus_xx),
    }
}

fn cached(c: &Consts, p: &P3) -> Cached {
    Cached {
        ypx: p.y.add(p.x),
        ymx: p.y.sub(p.x),
        z: p.z,
        t2d: p.t.mul(c.d2),
    }
}

#[inline(always)]
fn add(p: &P3, q: &Cached) -> P1 {
    let a = p.y.sub_lazy(p.x).mul(q.ymx);
    let b = p.y.add(p.x).mul(q.ypx);
    let cc = p.t.mul(q.t2d);
    let zz = p.z.mul(q.z);
    let d = zz.add(zz);
    P1 {
        x: b.sub_lazy(a),
        y: b.add(a),
        z: d.add(cc),
        t: d.sub_lazy(cc),
    }
}

#[inline(always)]
fn sub(p: &P3, q: &Cached) -> P1 {
    let a = p.y.sub_lazy(p.x).mul(q.ypx);
    let b = p.y.add(p.x).mul(q.ymx);
    let cc = p.t.mul(q.t2d);
    let zz = p.z.mul(q.z);
    let d = zz.add(zz);
    P1 {
        x: b.sub_lazy(a),
        y: b.add(a),
        z: d.sub_lazy(cc),
        t: d.add(cc),
    }
}

#[inline(always)]
fn madd(p: &P3, q: &Niels) -> P1 {
    let a = p.y.sub_lazy(p.x).mul(q.ymx);
    let b = p.y.add(p.x).mul(q.ypx);
    let cc = p.t.mul(q.xy2d);
    let d = p.z.add(p.z);
    P1 {
        x: b.sub_lazy(a),
        y: b.add(a),
        z: d.add(cc),
        t: d.sub_lazy(cc),
    }
}

#[inline(always)]
fn msub(p: &P3, q: &Niels) -> P1 {
    let a = p.y.sub_lazy(p.x).mul(q.ypx);
    let b = p.y.add(p.x).mul(q.ymx);
    let cc = p.t.mul(q.xy2d);
    let d = p.z.add(p.z);
    P1 {
        x: b.sub_lazy(a),
        y: b.add(a),
        z: d.sub_lazy(cc),
        t: d.add(cc),
    }
}

impl P3 {
    pub fn negate(&self) -> P3 {
        P3 {
            x: self.x.neg(),
            y: self.y,
            z: self.z,
            t: self.t.neg(),
        }
    }

    /// Whether `[8]P` is the identity.
    pub fn is_small_order(&self) -> bool {
        let p = to_p2(self);
        let p = p1_to_p2(&dbl(&p));
        let p = p1_to_p2(&dbl(&p));
        let p = p1_to_p2(&dbl(&p));
        p.x.is_zero()
    }

    /// The affine y coordinate.
    pub fn y_affine(&self) -> [u8; 32] {
        self.y.mul(self.z.invert()).to_bytes()
    }

    /// RFC 8032 §5.1.2 encoding.
    pub fn compress(&self) -> [u8; 32] {
        let zi = self.z.invert();
        let x = self.x.mul(zi);
        let mut out = self.y.mul(zi).to_bytes();
        out[31] |= x.is_negative() << 7;
        out
    }
}

/// Width-`w` non-adjacent form of a 256-bit little-endian scalar.
fn wnaf(s: &[u8; 32], w: u32) -> [i8; 257] {
    let mut k = [0u64; 5];
    for (i, c) in s.as_chunks::<8>().0.iter().enumerate() {
        k[i] = u64::from_le_bytes(*c);
    }
    let mut out = [0i8; 257];
    let full = 1i64 << w;
    let half = full >> 1;
    let mut i = 0;
    while k.iter().any(|&x| x != 0) && i < 257 {
        if k[0] & 1 == 1 {
            let mut d = (k[0] & (full as u64 - 1)) as i64;
            if d >= half {
                d -= full;
            }
            out[i] = d as i8;
            // k -= d
            if d > 0 {
                let (r, mut borrow) = k[0].overflowing_sub(d as u64);
                k[0] = r;
                for x in k.iter_mut().skip(1) {
                    if !borrow {
                        break;
                    }
                    let (r, b) = x.overflowing_sub(1);
                    *x = r;
                    borrow = b;
                }
            } else {
                let (r, mut carry) = k[0].overflowing_add((-d) as u64);
                k[0] = r;
                for x in k.iter_mut().skip(1) {
                    if !carry {
                        break;
                    }
                    let (r, c) = x.overflowing_add(1);
                    *x = r;
                    carry = c;
                }
            }
        }
        // k >>= 1
        for j in 0..4 {
            k[j] = (k[j] >> 1) | (k[j + 1] << 63);
        }
        k[4] >>= 1;
        i += 1;
    }
    out
}

/// `[a]P + [b]B`, encoded. Variable-time: public inputs only.
pub fn double_scalar_mul_vartime(a: &[u8; 32], p: &P3, b: &[u8; 32]) -> [u8; 32] {
    #[cfg(target_arch = "x86_64")]
    if crate::field::has_bmi2() {
        // SAFETY: BMI2 is available.
        return unsafe { double_scalar_mul_bmi2(a, p, b) };
    }
    double_scalar_mul_impl(a, p, b)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "bmi2")]
unsafe fn double_scalar_mul_bmi2(a: &[u8; 32], p: &P3, b: &[u8; 32]) -> [u8; 32] {
    double_scalar_mul_impl(a, p, b)
}

#[inline(always)]
fn double_scalar_mul_impl(a: &[u8; 32], p: &P3, b: &[u8; 32]) -> [u8; 32] {
    let c = consts();
    let na = wnaf(a, 5);
    let nb = wnaf(b, 8);
    // Odd multiples P, 3P, …, 15P.
    let mut tbl = [cached(c, p); 8];
    let p2 = cached(c, &to_p3(dbl(&to_p2(p))));
    let mut cur = *p;
    for slot in tbl.iter_mut().skip(1) {
        cur = to_p3(add(&cur, &p2));
        *slot = cached(c, &cur);
    }
    let mut top = 256;
    while top > 0 && na[top] == 0 && nb[top] == 0 {
        top -= 1;
    }
    let mut r = P2 {
        x: Fe::ZERO,
        y: Fe::ONE,
        z: Fe::ONE,
    };
    let mut i = top as isize;
    while i >= 0 {
        let t = dbl(&r);
        let (da, db) = (na[i as usize], nb[i as usize]);
        if da == 0 && db == 0 {
            r = p1_to_p2(&t);
        } else {
            let mut q = to_p3(t);
            if da > 0 {
                q = to_p3(add(&q, &tbl[(da / 2) as usize]));
            } else if da < 0 {
                q = to_p3(sub(&q, &tbl[(-da / 2) as usize]));
            }
            if db > 0 {
                q = to_p3(madd(&q, &c.base[(db / 2) as usize]));
            } else if db < 0 {
                q = to_p3(msub(&q, &c.base[(-db / 2) as usize]));
            }
            r = to_p2(&q);
        }
        i -= 1;
    }
    let zi = r.z.invert();
    let x = r.x.mul(zi);
    let mut out = r.y.mul(zi).to_bytes();
    out[31] |= x.is_negative() << 7;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use purecrypto::ec::edwards25519::hazmat::{EdwardsPoint, Scalar};

    #[test]
    fn constants() {
        let c = consts();
        assert!(c.sqrt_m1.square().ct_eq(Fe::ONE.neg()));
        let k = Fe([121666, 0, 0, 0, 0]);
        assert!(c.d.mul(k).ct_eq(Fe([121665, 0, 0, 0, 0]).neg()));
    }

    #[test]
    #[ignore]
    fn bench_parts() {
        let sp = Scalar::from_bytes_mod_order(&[9; 64]);
        let enc = EdwardsPoint::mul_base(&sp).compress();
        let n = 5000;
        let t = std::time::Instant::now();
        for _ in 0..n {
            std::hint::black_box(decompress(std::hint::black_box(&enc), true));
        }
        eprintln!(
            "decompress: {:.2} us",
            t.elapsed().as_nanos() as f64 / n as f64 / 1000.0
        );
        let p = decompress(&enc, true).unwrap();
        let t = std::time::Instant::now();
        for _ in 0..n {
            std::hint::black_box(p.is_small_order());
        }
        eprintln!(
            "small order: {:.2} us",
            t.elapsed().as_nanos() as f64 / n as f64 / 1000.0
        );
        let a = [0x5au8; 32];
        let mut b = [0xa5u8; 32];
        b[31] = 0x0f;
        let t = std::time::Instant::now();
        for _ in 0..n {
            std::hint::black_box(wnaf(std::hint::black_box(&a), 5));
            std::hint::black_box(wnaf(std::hint::black_box(&b), 8));
        }
        eprintln!(
            "wnaf x2: {:.2} us",
            t.elapsed().as_nanos() as f64 / n as f64 / 1000.0
        );
        let t = std::time::Instant::now();
        for _ in 0..n {
            std::hint::black_box(double_scalar_mul_vartime(&a, &p, &b));
        }
        eprintln!(
            "double scalar: {:.2} us",
            t.elapsed().as_nanos() as f64 / n as f64 / 1000.0
        );
    }

    #[test]
    fn small_order_list() {
        let c = consts();
        assert_eq!(c.small_order.len(), 14);
        for e in &c.small_order {
            let p = decompress(e, false).expect("decodes");
            assert!(p.is_small_order());
        }
        // T8 has order exactly 8: [4]T8 is not the identity.
        let t = decompress(&T8, true).unwrap();
        let mut q = to_p2(&t);
        for _ in 0..2 {
            q = p1_to_p2(&dbl(&q));
        }
        assert!(!q.y.sub(q.z).is_zero());
        // Points of large order are not listed.
        let b = decompress(
            &[
                0x58, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
                0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
                0x66, 0x66, 0x66, 0x66,
            ],
            true,
        )
        .unwrap();
        assert!(!b.is_small_order());
        assert!(!is_small_order_encoding(&b.compress()));
    }

    #[test]
    fn matches_hazmat() {
        let c = consts();
        let _ = c;
        for seed in 1u8..6 {
            let sa = Scalar::from_bytes_mod_order(&[seed; 64]);
            let sb = Scalar::from_bytes_mod_order(&[seed.wrapping_mul(77); 64]);
            let sp = Scalar::from_bytes_mod_order(&[seed ^ 0x55; 64]);
            let p = EdwardsPoint::mul_base(&sp);
            let want = p.mul(&sa).add(&EdwardsPoint::mul_base(&sb)).compress();
            let mine = decompress(&p.compress(), true).unwrap();
            assert_eq!(mine.compress(), p.compress());
            let got = double_scalar_mul_vartime(&sa.to_bytes(), &mine, &sb.to_bytes());
            assert_eq!(got, want, "seed {seed}");
        }
    }
}
