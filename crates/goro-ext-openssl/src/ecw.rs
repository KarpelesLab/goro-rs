//! Constant-time short-Weierstrass arithmetic (y^2 = x^3 + ax + b mod p) for
//! curves purecrypto has no `CurveId` for: the legacy named curves outside
//! its set and explicit-parameter curves from `openssl_pkey_new()` or
//! ECParameters.
//!
//! purecrypto's own runtime-parameter engine (`ec::weierstrass::Engine`) is
//! crate-private, so this mirrors its design on the public constant-time
//! bignum layer: field elements live in Montgomery form in a
//! `BoxedMontModulus`, points are projective and combined with the
//! Renes-Costello-Batina complete addition formulas for arbitrary `a`
//! (Algorithm 1 of eprint 2015/1060), which are branch-free and exception
//! free for points of odd order (so for every point of the prime-order
//! subgroup, including the identity). Scalar multiplication is a fixed
//! double-and-add-always ladder over the public bit width of the order,
//! with a constant-time select of the sum.

use crate::bn::Bn;
use purecrypto::bignum::{BoxedMontModulus, BoxedUint};
use purecrypto::ct::Choice;

pub struct Curve {
    fp: BoxedMontModulus,
    /// p - 2 (Fermat inversion exponent; public).
    pm2: BoxedUint,
    a: BoxedUint,
    b3: BoxedUint,
    one: BoxedUint,
    zero: BoxedUint,
    limbs: usize,
    len: usize,
}

/// Projective point (X:Y:Z) in Montgomery form; Z = 0 is the identity.
#[derive(Clone)]
pub struct Proj {
    x: BoxedUint,
    y: BoxedUint,
    z: BoxedUint,
}

impl Curve {
    /// `None` unless `p` is odd and at least 3.
    pub fn new(p: &Bn, a: &Bn, b: &Bn) -> Option<Curve> {
        if p.is_even() || p.bits() < 2 {
            return None;
        }
        let pb = p.to_box();
        let fp = BoxedMontModulus::new(&pb);
        let limbs = fp.limbs();
        let am = fp.to_mont(&fp.reduce(&a.to_box()));
        let bm = fp.to_mont(&fp.reduce(&b.to_box()));
        let b2 = fp.add_mod(&bm, &bm);
        let b3 = fp.add_mod(&b2, &bm);
        let one = fp.to_mont(&BoxedUint::from_u64(1));
        Some(Curve {
            pm2: pb.sub(&BoxedUint::from_u64(2)),
            fp,
            a: am,
            b3,
            one,
            zero: BoxedUint::zero(limbs),
            limbs,
            len: p.byte_len(),
        })
    }

    fn mul(&self, a: &BoxedUint, b: &BoxedUint) -> BoxedUint {
        self.fp.mont_mul(a, b)
    }

    fn add(&self, a: &BoxedUint, b: &BoxedUint) -> BoxedUint {
        self.fp.add_mod(a, b)
    }

    fn sub(&self, a: &BoxedUint, b: &BoxedUint) -> BoxedUint {
        self.fp.sub_mod(a, b)
    }

    pub fn identity(&self) -> Proj {
        Proj {
            x: self.zero.clone(),
            y: self.one.clone(),
            z: self.zero.clone(),
        }
    }

    /// Affine (public) coordinates to a projective point. Coordinates are
    /// reduced mod p.
    pub fn point(&self, x: &Bn, y: &Bn) -> Proj {
        Proj {
            x: self.fp.to_mont(&self.fp.reduce(&x.to_box())),
            y: self.fp.to_mont(&self.fp.reduce(&y.to_box())),
            z: self.one.clone(),
        }
    }

    /// Complete addition (RCB Algorithm 1, generic a).
    pub fn padd(&self, p: &Proj, q: &Proj) -> Proj {
        let (x1, y1, z1) = (&p.x, &p.y, &p.z);
        let (x2, y2, z2) = (&q.x, &q.y, &q.z);
        let mut t0 = self.mul(x1, x2);
        let mut t1 = self.mul(y1, y2);
        let mut t2 = self.mul(z1, z2);
        let mut t3 = self.add(x1, y1);
        let mut t4 = self.add(x2, y2);
        t3 = self.mul(&t3, &t4);
        t4 = self.add(&t0, &t1);
        t3 = self.sub(&t3, &t4);
        t4 = self.add(x1, z1);
        let mut t5 = self.add(x2, z2);
        t4 = self.mul(&t4, &t5);
        t5 = self.add(&t0, &t2);
        t4 = self.sub(&t4, &t5);
        t5 = self.add(y1, z1);
        let mut x3 = self.add(y2, z2);
        t5 = self.mul(&t5, &x3);
        x3 = self.add(&t1, &t2);
        t5 = self.sub(&t5, &x3);
        let mut z3 = self.mul(&self.a, &t4);
        x3 = self.mul(&self.b3, &t2);
        z3 = self.add(&x3, &z3);
        x3 = self.sub(&t1, &z3);
        z3 = self.add(&t1, &z3);
        let mut y3 = self.mul(&x3, &z3);
        t1 = self.add(&t0, &t0);
        t1 = self.add(&t1, &t0);
        t2 = self.mul(&self.a, &t2);
        t4 = self.mul(&self.b3, &t4);
        t1 = self.add(&t1, &t2);
        t2 = self.sub(&t0, &t2);
        t2 = self.mul(&self.a, &t2);
        t4 = self.add(&t4, &t2);
        t0 = self.mul(&t1, &t4);
        y3 = self.add(&y3, &t0);
        t0 = self.mul(&t5, &t4);
        x3 = self.mul(&t3, &x3);
        x3 = self.sub(&x3, &t0);
        t0 = self.mul(&t3, &t1);
        z3 = self.mul(&t5, &z3);
        z3 = self.add(&z3, &t0);
        Proj {
            x: x3,
            y: y3,
            z: z3,
        }
    }

    fn select(a: &Proj, b: &Proj, c: Choice) -> Proj {
        Proj {
            x: BoxedUint::conditional_select(&a.x, &b.x, c),
            y: BoxedUint::conditional_select(&a.y, &b.y, c),
            z: BoxedUint::conditional_select(&a.z, &b.z, c),
        }
    }

    /// `k * P` for a (possibly secret) scalar `k < 2^bits`, `bits` public:
    /// `bits` doublings and `bits` additions whatever the value of `k`.
    pub fn smul(&self, k: &BoxedUint, bits: usize, p: &Proj) -> Proj {
        let limbs = k.as_limbs();
        let mut r = self.identity();
        for i in (0..bits.max(1)).rev() {
            r = self.padd(&r, &r);
            let s = self.padd(&r, p);
            let bit = (limbs.get(i / 64).copied().unwrap_or(0) >> (i % 64)) & 1;
            r = Self::select(&s, &r, Choice::from(bit as u8));
        }
        r
    }

    /// Affine coordinates (plain, minimal big-endian); `None` for the
    /// identity. The result is public (a public key, a signature's `r`
    /// before reduction, an ECDH output), so only the identity verdict is
    /// branched on.
    pub fn affine(&self, p: &Proj) -> Option<(Bn, Bn)> {
        let z = self.fp.from_mont(&p.z);
        if bool::from(z.ct_is_zero()) {
            return None;
        }
        let zi = self.fp.to_mont(&self.fp.pow(&z, &self.pm2));
        let x = self.fp.from_mont(&self.mul(&p.x, &zi));
        let y = self.fp.from_mont(&self.mul(&p.y, &zi));
        Some((Bn::from_box(&x), Bn::from_box(&y)))
    }

    /// Affine x coordinate padded to the field width.
    pub fn affine_x_bytes(&self, p: &Proj) -> Option<Vec<u8>> {
        let (x, _) = self.affine(p)?;
        Some(x.pad(self.len))
    }

    pub fn limbs(&self) -> usize {
        self.limbs
    }
}
