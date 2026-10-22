//! Prime-field elliptic curve groups (y^2 = x^3 + ax + b mod p), with
//! ECDSA / SM2 / ECDH.
//!
//! Private-key operations run on purecrypto's constant-time code: its boxed
//! ECDSA/ECDH keys for the curves it implements (`CurveId`), and otherwise
//! the constant-time engine in [`crate::ecw`] (built on purecrypto's
//! Montgomery arithmetic) with scalar arithmetic in [`crate::bn::Modulus`].
//! Public-only work (point decoding and validation, cofactor guessing) uses
//! puremp.

use crate::bn::{self, Bn, Modulus, public};
use crate::ec_curves::CURVES;
use crate::ecw::{Curve, Proj};
use purecrypto::bignum::BoxedUint;
use purecrypto::ec::{
    BoxedEcdhPrivateKey, BoxedEcdsaPrivateKey, BoxedEcdsaPublicKey, BoxedEcdsaSignature, CurveId,
};
use puremp::Nat;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub p: Bn,
    pub a: Bn,
    pub b: Bn,
    pub gx: Bn,
    pub gy: Bn,
    pub n: Bn,
    pub h: Bn,
    pub seed: Option<Vec<u8>>,
    /// Short name of the matching built-in curve.
    pub name: Option<&'static str>,
    pub oid: Option<&'static str>,
    /// True when the key should be encoded with explicit parameters.
    pub explicit: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Point {
    Inf,
    Aff(Bn, Bn),
}

fn hex(s: &str) -> Bn {
    Bn::from_hex(s)
}

/// Names returned by openssl_get_curve_names() (built-in prime curves).
pub fn curve_names() -> Vec<&'static str> {
    CURVES.iter().map(|c| c.sn).collect()
}

impl Group {
    pub fn by_name(sn: &str) -> Option<Group> {
        let c = CURVES.iter().find(|c| c.sn == sn)?;
        Some(Self::from_builtin(c))
    }

    pub fn by_oid(oid: &str) -> Option<Group> {
        let c = CURVES.iter().find(|c| c.oid == oid)?;
        Some(Self::from_builtin(c))
    }

    fn from_builtin(c: &'static crate::ec_curves::Curve) -> Group {
        Group {
            p: hex(c.p),
            a: hex(c.a),
            b: hex(c.b),
            gx: hex(c.gx),
            gy: hex(c.gy),
            n: hex(c.n),
            h: Bn::from_u64(c.h as u64),
            seed: if c.seed.is_empty() {
                None
            } else {
                hex_bytes(c.seed)
            },
            name: Some(c.sn),
            oid: Some(c.oid),
            explicit: false,
        }
    }

    /// Match explicit parameters against the built-in curves
    /// (EC_GROUP_check_named_curve).
    pub fn identify(&mut self) {
        for c in CURVES {
            if hex(c.p) == self.p
                && hex(c.a) == self.a
                && hex(c.b) == self.b
                && hex(c.gx) == self.gx
                && hex(c.gy) == self.gy
                && hex(c.n) == self.n
            {
                self.name = Some(c.sn);
                self.oid = Some(c.oid);
                return;
            }
        }
    }

    /// The purecrypto curve for this group, when it implements it.
    pub fn curve_id(&self) -> Option<CurveId> {
        Some(match self.name? {
            "prime256v1" => CurveId::P256,
            "secp384r1" => CurveId::P384,
            "secp521r1" => CurveId::P521,
            "secp256k1" => CurveId::Secp256k1,
            "SM2" => CurveId::Sm2p256v1,
            "brainpoolP224r1" => CurveId::BrainpoolP224r1,
            "brainpoolP256r1" => CurveId::BrainpoolP256r1,
            "brainpoolP320r1" => CurveId::BrainpoolP320r1,
            "brainpoolP384r1" => CurveId::BrainpoolP384r1,
            "brainpoolP512r1" => CurveId::BrainpoolP512r1,
            "secp160k1" => CurveId::Secp160k1,
            "secp160r1" => CurveId::Secp160r1,
            "secp160r2" => CurveId::Secp160r2,
            "secp192k1" => CurveId::Secp192k1,
            "prime192v1" => CurveId::P192,
            "secp224k1" => CurveId::Secp224k1,
            "secp224r1" => CurveId::P224,
            _ => return None,
        })
    }

    fn pc_private(&self, d: &Bn) -> Option<(CurveId, Vec<u8>)> {
        let id = self.curve_id()?;
        if d.byte_len() > id.order_len() {
            return None;
        }
        Some((id, d.pad(id.order_len())))
    }

    fn pc_public(&self, q: &Point) -> Option<(CurveId, BoxedEcdsaPublicKey)> {
        let id = self.curve_id()?;
        let k = BoxedEcdsaPublicKey::from_sec1(id, &self.encode(q, false)).ok()?;
        Some((id, k))
    }

    /// Field element size in bytes.
    pub fn field_len(&self) -> usize {
        self.p.byte_len()
    }

    pub fn order_bits(&self) -> usize {
        self.n.bits()
    }

    pub fn generator(&self) -> Point {
        Point::Aff(self.gx.clone(), self.gy.clone())
    }

    fn engine(&self) -> Option<Curve> {
        Curve::new(&self.p, &self.a, &self.b)
    }

    fn order(&self) -> Option<Modulus> {
        Modulus::new(&self.n)
    }

    fn proj(&self, c: &Curve, pt: &Point) -> Proj {
        match pt {
            Point::Inf => c.identity(),
            Point::Aff(x, y) => c.point(x, y),
        }
    }

    fn from_proj(c: &Curve, p: &Proj) -> Point {
        match c.affine(p) {
            Some((x, y)) => Point::Aff(x, y),
            None => Point::Inf,
        }
    }

    /// `k * pt` in constant time in `k`. The scalar is reduced mod the
    /// order (points live in the order-n subgroup), so the ladder runs over
    /// the public width of n.
    fn smul(&self, c: &Curve, k: &BoxedUint, pt: &Proj) -> Proj {
        match self.order() {
            Some(nm) => c.smul(&nm.reduce(k), self.n.bits(), pt),
            None => c.smul(k, 64 * k.limbs().max(c.limbs()), pt),
        }
    }

    /// x^3 + ax + b mod p (public values).
    fn rhs(&self, x: &Nat) -> Nat {
        let p = self.p.nat();
        let x3 = x.mul(x).mul(x);
        public::rem(&x3.add(&self.a.nat().mul(x)).add(&self.b.nat()), &p)
    }

    pub fn on_curve(&self, pt: &Point) -> bool {
        match pt {
            Point::Inf => true,
            Point::Aff(x, y) => {
                if self.p.is_zero() || x >= &self.p || y >= &self.p {
                    return false;
                }
                let p = self.p.nat();
                let y = y.nat();
                public::rem(&y.mul(&y), &p) == self.rhs(&x.nat())
            }
        }
    }

    /// Point to octet string (SEC1).
    pub fn encode(&self, pt: &Point, compressed: bool) -> Vec<u8> {
        match pt {
            Point::Inf => vec![0],
            Point::Aff(x, y) => {
                let fl = self.field_len();
                let mut out = Vec::with_capacity(1 + 2 * fl);
                if compressed {
                    out.push(if y.is_odd() { 3 } else { 2 });
                    out.extend(x.pad(fl));
                } else {
                    out.push(4);
                    out.extend(x.pad(fl));
                    out.extend(y.pad(fl));
                }
                out
            }
        }
    }

    /// Octet string to point (EC_POINT_oct2point); validates the point.
    pub fn decode(&self, b: &[u8]) -> Option<Point> {
        let fl = self.field_len();
        if b.is_empty() || self.p.is_zero() {
            return None;
        }
        let pt = match b[0] {
            0 if b.len() == 1 => return Some(Point::Inf),
            4 | 6 | 7 if b.len() == 1 + 2 * fl => {
                Point::Aff(Bn::from_be(&b[1..1 + fl]), Bn::from_be(&b[1 + fl..]))
            }
            2 | 3 if b.len() == 1 + fl => {
                let x = Bn::from_be(&b[1..]);
                if x >= self.p {
                    return None;
                }
                let rhs = self.rhs(&x.nat());
                let y = self.sqrt(&rhs)?;
                let y = if y.bit(0) == (b[0] == 3) {
                    y
                } else {
                    public::sub(&self.p.nat(), &y)
                };
                Point::Aff(x, Bn::from_nat(&y))
            }
            _ => return None,
        };
        if !self.on_curve(&pt) {
            return None;
        }
        Some(pt)
    }

    /// Modular square root of a public value (Tonelli-Shanks).
    fn sqrt(&self, a: &Nat) -> Option<Nat> {
        let p = self.p.nat();
        if a.is_zero() {
            return Some(Nat::zero());
        }
        let one = Nat::one();
        let pm1 = public::sub(&p, &one);
        let half = pm1.shr(1);
        if a.modpow(&half, &p) != one {
            return None;
        }
        if public::rem(&p, &Nat::from_u64(4)) == Nat::from_u64(3) {
            return Some(a.modpow(&p.add(&one).shr(2), &p));
        }
        let s = pm1.trailing_zeros();
        let q = pm1.shr(s);
        let mut z = Nat::from_u64(2);
        while z.modpow(&half, &p) != pm1 {
            z = z.add(&one);
            if z >= p {
                return None;
            }
        }
        let mut m = s;
        let mut c = z.modpow(&q, &p);
        let mut t = a.modpow(&q, &p);
        let mut r = a.modpow(&q.add(&one).shr(1), &p);
        while t != one {
            let mut i = 0u64;
            let mut tt = t.clone();
            while tt != one {
                tt = public::rem(&tt.mul(&tt), &p);
                i += 1;
                if i == m {
                    return None;
                }
            }
            let b = c.modpow(&one.shl(m - i - 1), &p);
            m = i;
            c = public::rem(&b.mul(&b), &p);
            t = public::rem(&t.mul(&c), &p);
            r = public::rem(&r.mul(&b), &p);
        }
        Some(r)
    }

    /// Fresh secret scalar in [1, n).
    pub fn random_scalar(&self) -> BoxedUint {
        bn::random_below(&self.n)
    }

    pub fn pub_from_priv(&self, d: &Bn) -> Point {
        if let Some((id, db)) = self.pc_private(d) {
            if let Ok(k) = BoxedEcdsaPrivateKey::from_bytes(id, &db) {
                if let Some(p) = self.decode(&k.public_key().to_sec1()) {
                    return p;
                }
            }
        }
        let Some(c) = self.engine() else {
            return Point::Inf;
        };
        let g = self.proj(&c, &self.generator());
        Self::from_proj(&c, &self.smul(&c, &d.to_box(), &g))
    }

    /// Truncate a hash to the bit length of the order (ECDSA e value).
    fn hash_to_int(&self, hash: &[u8]) -> BoxedUint {
        let nbits = self.n.bits();
        let e = BoxedUint::from_be_bytes(hash);
        let hbits = hash.len() * 8;
        if hbits > nbits {
            e.shr_bits(hbits - nbits)
        } else {
            e
        }
    }

    /// ECDSA sign, returning (r, s).
    pub fn ecdsa_sign(&self, d: &Bn, hash: &[u8]) -> Option<(Bn, Bn)> {
        if let Some((id, db)) = self.pc_private(d) {
            if let Ok(k) = BoxedEcdsaPrivateKey::from_bytes(id, &db) {
                if let Ok(sig) = k.sign_prehash::<purecrypto::hash::Sha256>(hash) {
                    return Some((Bn::from_be(&sig.r_bytes(id)), Bn::from_be(&sig.s_bytes(id))));
                }
            }
        }
        // Constant-time generic path: k*G on the ecw engine, k^-1 by the
        // constant-time binary xgcd, s = k^-1 (e + r d) in Montgomery form.
        let c = self.engine()?;
        let nm = self.order()?;
        let e = nm.reduce(&self.hash_to_int(hash));
        let d = nm.reduce(&d.to_box());
        if bool::from(d.ct_is_zero()) {
            return None;
        }
        let g = self.proj(&c, &self.generator());
        for _ in 0..64 {
            let k = self.random_scalar();
            let Some((x, _)) = c.affine(&c.smul(&k, self.n.bits(), &g)) else {
                continue;
            };
            let r = nm.reduce(&x.to_box());
            if r.is_zero() {
                continue;
            }
            let kinv = nm.inv(&k)?;
            let s = nm.mul(&kinv, &nm.add(&e, &nm.mul(&r, &d)));
            if s.is_zero() {
                continue;
            }
            return Some((Bn::from_box(&r), Bn::from_box(&s)));
        }
        None
    }

    /// x coordinate of u1*G + u2*Q (public scalars).
    fn double_mul_x(&self, u1: &BoxedUint, u2: &BoxedUint, q: &Point) -> Option<Bn> {
        let c = self.engine()?;
        let g = self.proj(&c, &self.generator());
        let qp = self.proj(&c, q);
        let bits = self.n.bits();
        let sum = c.padd(&c.smul(u1, bits, &g), &c.smul(u2, bits, &qp));
        c.affine(&sum).map(|(x, _)| x)
    }

    pub fn ecdsa_verify(&self, q: &Point, hash: &[u8], r: &Bn, s: &Bn) -> bool {
        if r.is_zero() || s.is_zero() || r >= &self.n || s >= &self.n {
            return false;
        }
        if let Some((_, k)) = self.pc_public(q) {
            let sig = BoxedEcdsaSignature::from_components(r.to_box(), s.to_box());
            return k.verify_prehash(hash, &sig).is_ok();
        }
        // Everything below is public.
        let Some(nm) = self.order() else {
            return false;
        };
        let Some(w) = public::inv(&s.nat(), &self.n.nat()) else {
            return false;
        };
        let w = Bn::from_nat(&w).to_box();
        let e = self.hash_to_int(hash);
        let u1 = nm.mul(&e, &w);
        let u2 = nm.mul(&r.to_box(), &w);
        match self.double_mul_x(&u1, &u2, q) {
            Some(x) => Bn::from_box(&nm.reduce(&x.to_box())) == *r,
            None => false,
        }
    }

    /// SM2 signature over a digest (no Z prefix, as OpenSSL 3 without an ID).
    /// purecrypto's `Sm2PrivateKey` always hashes Z_A in, so this runs on the
    /// constant-time generic path.
    pub fn sm2_sign(&self, d: &Bn, hash: &[u8]) -> Option<(Bn, Bn)> {
        let c = self.engine()?;
        let nm = self.order()?;
        let e = nm.reduce(&BoxedUint::from_be_bytes(hash));
        let d = nm.reduce(&d.to_box());
        let inv = nm.inv(&nm.add(&d, &BoxedUint::from_u64(1)))?;
        let g = self.proj(&c, &self.generator());
        for _ in 0..64 {
            let k = self.random_scalar();
            let Some((x1, _)) = c.affine(&c.smul(&k, self.n.bits(), &g)) else {
                continue;
            };
            let r = nm.add(&e, &x1.to_box());
            // r == 0 or r + k == n: public retry verdicts.
            if r.is_zero() || bool::from(nm.add(&r, &k).ct_is_zero()) {
                continue;
            }
            let s = nm.mul(&inv, &nm.sub(&k, &nm.mul(&r, &d)));
            if s.is_zero() {
                continue;
            }
            return Some((Bn::from_box(&r), Bn::from_box(&s)));
        }
        None
    }

    pub fn sm2_verify(&self, q: &Point, hash: &[u8], r: &Bn, s: &Bn) -> bool {
        if r.is_zero() || s.is_zero() || r >= &self.n || s >= &self.n {
            return false;
        }
        let Some(nm) = self.order() else {
            return false;
        };
        let e = nm.reduce(&BoxedUint::from_be_bytes(hash));
        let t = nm.add(&r.to_box(), &s.to_box());
        if t.is_zero() {
            return false;
        }
        match self.double_mul_x(&s.to_box(), &t, q) {
            Some(x1) => Bn::from_box(&nm.add(&e, &x1.to_box())) == *r,
            None => false,
        }
    }

    /// ECDH shared secret: x coordinate of d*Q, padded to the field size.
    pub fn ecdh(&self, d: &Bn, q: &Point) -> Option<Vec<u8>> {
        if let (Some((id, db)), Some((_, peer))) = (self.pc_private(d), self.pc_public(q)) {
            let k = BoxedEcdhPrivateKey::from_bytes(id, &db).ok()?;
            return k.diffie_hellman(&peer).ok();
        }
        let c = self.engine()?;
        let qp = self.proj(&c, q);
        c.affine_x_bytes(&self.smul(&c, &d.to_box(), &qp))
    }

    /// Guess the cofactor (ec_guess_cofactor): h = (p + 1 + n/2) / n.
    pub fn guess_cofactor(&mut self) {
        if self.n.is_zero() {
            return;
        }
        let n = self.n.nat();
        let v = self.p.nat().add(&Nat::one()).add(&n.shr(1));
        self.h = Bn::from_nat(&public::div(&v, &n));
    }
}

fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same group with its name stripped, so every operation takes the
    /// generic constant-time engine instead of purecrypto's fixed curve.
    fn generic(name: &str) -> (Group, Group) {
        let g = Group::by_name(name).unwrap();
        let mut e = g.clone();
        e.name = None;
        e.oid = None;
        (g, e)
    }

    #[test]
    fn engine_matches_purecrypto_curves() {
        for name in ["prime256v1", "secp384r1", "secp521r1", "secp256k1", "SM2"] {
            let (named, eng) = generic(name);
            for _ in 0..4 {
                let d = Bn::from_box(&named.random_scalar());
                let pubk = named.pub_from_priv(&d);
                assert_eq!(pubk, eng.pub_from_priv(&d), "{name}");
                let peer = Bn::from_box(&named.random_scalar());
                let q = named.pub_from_priv(&peer);
                assert_eq!(named.ecdh(&d, &q), eng.ecdh(&d, &q), "{name}");
                let hash = [0x5au8; 32];
                let (r, s) = eng.ecdsa_sign(&d, &hash).unwrap();
                assert!(named.ecdsa_verify(&pubk, &hash, &r, &s), "{name}");
                let (r, s) = named.ecdsa_sign(&d, &hash).unwrap();
                assert!(eng.ecdsa_verify(&pubk, &hash, &r, &s), "{name}");
                assert!(!eng.ecdsa_verify(&pubk, &[0x5bu8; 32], &r, &s), "{name}");
            }
        }
    }

    #[test]
    fn edge_scalars() {
        let (named, eng) = generic("prime256v1");
        assert_eq!(eng.pub_from_priv(&Bn::from_u64(1)), eng.generator());
        let nm1 = Bn::from_nat(&public::sub(&named.n.nat(), &Nat::one()));
        assert_eq!(named.pub_from_priv(&nm1), eng.pub_from_priv(&nm1));
        assert_eq!(eng.pub_from_priv(&named.n), Point::Inf);
        assert_eq!(eng.pub_from_priv(&Bn::default()), Point::Inf);
    }

    #[test]
    fn sm2_roundtrip_and_compressed_points() {
        let g = Group::by_name("SM2").unwrap();
        let d = Bn::from_box(&g.random_scalar());
        let q = g.pub_from_priv(&d);
        let (r, s) = g.sm2_sign(&d, &[7u8; 32]).unwrap();
        assert!(g.sm2_verify(&q, &[7u8; 32], &r, &s));
        assert!(!g.sm2_verify(&q, &[8u8; 32], &r, &s));
        for name in ["secp112r1", "prime239v1", "brainpoolP256t1", "SM2"] {
            let g = Group::by_name(name).unwrap();
            let q = g.pub_from_priv(&Bn::from_box(&g.random_scalar()));
            assert_eq!(g.decode(&g.encode(&q, true)), Some(q), "{name}");
        }
    }
}
