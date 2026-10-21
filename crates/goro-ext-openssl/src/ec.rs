//! Generic short-Weierstrass elliptic curve arithmetic over prime fields
//! (y^2 = x^3 + ax + b mod p) using big integers, plus ECDSA/ECDH helpers.

use crate::ec_curves::CURVES;
use num_bigint_dig::BigUint;
use num_integer::Integer;
use num_traits::{One, Zero};
use purecrypto::bignum::BoxedUint;
use purecrypto::ec::{
    BoxedEcdhPrivateKey, BoxedEcdsaPrivateKey, BoxedEcdsaPublicKey, BoxedEcdsaSignature, CurveId,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub p: BigUint,
    pub a: BigUint,
    pub b: BigUint,
    pub gx: BigUint,
    pub gy: BigUint,
    pub n: BigUint,
    pub h: BigUint,
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
    Aff(BigUint, BigUint),
}

fn hex(s: &str) -> BigUint {
    BigUint::parse_bytes(s.as_bytes(), 16).unwrap_or_default()
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
            h: BigUint::from(c.h),
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

    fn pc_private(&self, d: &BigUint) -> Option<(CurveId, Vec<u8>)> {
        let id = self.curve_id()?;
        Some((id, pad(d, id.order_len())))
    }

    fn pc_public(&self, q: &Point) -> Option<(CurveId, BoxedEcdsaPublicKey)> {
        let id = self.curve_id()?;
        let k = BoxedEcdsaPublicKey::from_sec1(id, &self.encode(q, false)).ok()?;
        Some((id, k))
    }

    /// Field element size in bytes.
    pub fn field_len(&self) -> usize {
        self.p.bits().div_ceil(8)
    }

    pub fn order_bits(&self) -> usize {
        self.n.bits()
    }

    pub fn generator(&self) -> Point {
        Point::Aff(self.gx.clone(), self.gy.clone())
    }

    fn md(&self, x: &BigUint) -> BigUint {
        x % &self.p
    }

    fn sub(&self, a: &BigUint, b: &BigUint) -> BigUint {
        let a = a % &self.p;
        let b = b % &self.p;
        if a >= b { a - b } else { &self.p - (b - a) }
    }

    fn inv(&self, a: &BigUint) -> BigUint {
        // p is prime: a^(p-2)
        let e = &self.p - BigUint::from(2u32);
        a.modpow(&e, &self.p)
    }

    pub fn on_curve(&self, pt: &Point) -> bool {
        match pt {
            Point::Inf => true,
            Point::Aff(x, y) => {
                if x >= &self.p || y >= &self.p {
                    return false;
                }
                let lhs = self.md(&(y * y));
                let rhs = self.md(&(x * x * x + &self.a * x + &self.b));
                lhs == rhs
            }
        }
    }

    pub fn add(&self, p1: &Point, p2: &Point) -> Point {
        match (p1, p2) {
            (Point::Inf, q) | (q, Point::Inf) => q.clone(),
            (Point::Aff(x1, y1), Point::Aff(x2, y2)) => {
                if x1 == x2 {
                    if self.md(&(y1 + y2)).is_zero() {
                        return Point::Inf;
                    }
                    return self.double(p1);
                }
                let l = self.md(&(self.sub(y2, y1) * self.inv(&self.sub(x2, x1))));
                let x3 = self.sub(&self.sub(&(&l * &l), x1), x2);
                let y3 = self.sub(&(&l * self.sub(x1, &x3)), y1);
                Point::Aff(x3, y3)
            }
        }
    }

    pub fn double(&self, p1: &Point) -> Point {
        match p1 {
            Point::Inf => Point::Inf,
            Point::Aff(x, y) => {
                if y.is_zero() {
                    return Point::Inf;
                }
                let num = self.md(&(BigUint::from(3u32) * x * x + &self.a));
                let l = self.md(&(num * self.inv(&self.md(&(BigUint::from(2u32) * y)))));
                let x3 = self.sub(&(&l * &l), &self.md(&(BigUint::from(2u32) * x)));
                let y3 = self.sub(&(&l * self.sub(x, &x3)), y);
                Point::Aff(x3, y3)
            }
        }
    }

    /// Scalar multiplication using Jacobian coordinates.
    pub fn mul(&self, k: &BigUint, pt: &Point) -> Point {
        let (px, py) = match pt {
            Point::Inf => return Point::Inf,
            Point::Aff(x, y) => (x.clone(), y.clone()),
        };
        if k.is_zero() {
            return Point::Inf;
        }
        // Jacobian (X, Y, Z) with Z=0 meaning infinity
        let p = &self.p;
        let one = BigUint::one();
        let mut r: (BigUint, BigUint, BigUint) = (one.clone(), one.clone(), BigUint::zero());
        for byte in k.to_bytes_be() {
            for bit in (0..8).rev() {
                r = self.jdouble(&r);
                if (byte >> bit) & 1 == 1 {
                    r = self.jadd_affine(&r, &px, &py);
                }
            }
        }
        if r.2.is_zero() {
            return Point::Inf;
        }
        let zi = self.inv(&r.2);
        let zi2 = (&zi * &zi) % p;
        let zi3 = (&zi2 * &zi) % p;
        Point::Aff((&r.0 * zi2) % p, (&r.1 * zi3) % p)
    }

    fn jdouble(&self, q: &(BigUint, BigUint, BigUint)) -> (BigUint, BigUint, BigUint) {
        let p = &self.p;
        let (x, y, z) = q;
        if z.is_zero() || y.is_zero() {
            return (BigUint::one(), BigUint::one(), BigUint::zero());
        }
        let yy = (y * y) % p;
        let s = (BigUint::from(4u32) * x * &yy) % p;
        let zz = (z * z) % p;
        let m = (BigUint::from(3u32) * x * x + &self.a * ((&zz * &zz) % p)) % p;
        let x3 = self.sub(&((&m * &m) % p), &((BigUint::from(2u32) * &s) % p));
        let y3 = self.sub(
            &((&m * self.sub(&s, &x3)) % p),
            &((BigUint::from(8u32) * &yy * &yy) % p),
        );
        let z3 = (BigUint::from(2u32) * y * z) % p;
        (x3, y3, z3)
    }

    fn jadd_affine(
        &self,
        q: &(BigUint, BigUint, BigUint),
        x2: &BigUint,
        y2: &BigUint,
    ) -> (BigUint, BigUint, BigUint) {
        let p = &self.p;
        let (x1, y1, z1) = q;
        if z1.is_zero() {
            return (x2.clone(), y2.clone(), BigUint::one());
        }
        let z1z1 = (z1 * z1) % p;
        let u2 = (x2 * &z1z1) % p;
        let s2 = (y2 * z1 * &z1z1) % p;
        let h = self.sub(&u2, x1);
        let r = self.sub(&s2, y1);
        if h.is_zero() {
            if r.is_zero() {
                return self.jdouble(q);
            }
            return (BigUint::one(), BigUint::one(), BigUint::zero());
        }
        let hh = (&h * &h) % p;
        let hhh = (&hh * &h) % p;
        let v = (x1 * &hh) % p;
        let x3 = self.sub(
            &self.sub(&((&r * &r) % p), &hhh),
            &((BigUint::from(2u32) * &v) % p),
        );
        let y3 = self.sub(&((&r * self.sub(&v, &x3)) % p), &((y1 * &hhh) % p));
        let z3 = (z1 * &h) % p;
        (x3, y3, z3)
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
                    out.extend(pad(x, fl));
                } else {
                    out.push(4);
                    out.extend(pad(x, fl));
                    out.extend(pad(y, fl));
                }
                out
            }
        }
    }

    /// Octet string to point (EC_POINT_oct2point); validates the point.
    pub fn decode(&self, b: &[u8]) -> Option<Point> {
        let fl = self.field_len();
        if b.is_empty() {
            return None;
        }
        let pt = match b[0] {
            0 if b.len() == 1 => return Some(Point::Inf),
            4 | 6 | 7 if b.len() == 1 + 2 * fl => Point::Aff(
                BigUint::from_bytes_be(&b[1..1 + fl]),
                BigUint::from_bytes_be(&b[1 + fl..]),
            ),
            2 | 3 if b.len() == 1 + fl => {
                let x = BigUint::from_bytes_be(&b[1..]);
                if x >= self.p {
                    return None;
                }
                let rhs = self.md(&(&x * &x * &x + &self.a * &x + &self.b));
                let y = self.sqrt(&rhs)?;
                let y = if y.is_odd() == (b[0] == 3) {
                    y
                } else {
                    &self.p - y
                };
                Point::Aff(x, y)
            }
            _ => return None,
        };
        if !self.on_curve(&pt) {
            return None;
        }
        Some(pt)
    }

    /// Modular square root (Tonelli-Shanks).
    fn sqrt(&self, a: &BigUint) -> Option<BigUint> {
        let p = &self.p;
        if a.is_zero() {
            return Some(BigUint::zero());
        }
        let one = BigUint::one();
        let pm1 = p - &one;
        let legendre = a.modpow(&(&pm1 >> 1), p);
        if legendre != one {
            return None;
        }
        if (p % BigUint::from(4u32)) == BigUint::from(3u32) {
            return Some(a.modpow(&((p + &one) >> 2), p));
        }
        let mut q = pm1.clone();
        let mut s = 0u32;
        while q.is_even() {
            q >>= 1;
            s += 1;
        }
        let mut z = BigUint::from(2u32);
        while z.modpow(&(&pm1 >> 1), p) != pm1 {
            z += &one;
        }
        let mut m = s;
        let mut c = z.modpow(&q, p);
        let mut t = a.modpow(&q, p);
        let mut r = a.modpow(&((&q + &one) >> 1), p);
        while t != one {
            let mut i = 0u32;
            let mut tt = t.clone();
            while tt != one {
                tt = (&tt * &tt) % p;
                i += 1;
                if i == m {
                    return None;
                }
            }
            let b = c.modpow(&(BigUint::one() << (m - i - 1) as usize), p);
            m = i;
            c = (&b * &b) % p;
            t = (&t * &c) % p;
            r = (&r * &b) % p;
        }
        Some(r)
    }

    pub fn random_scalar(&self) -> BigUint {
        random_below(&self.n)
    }

    pub fn pub_from_priv(&self, d: &BigUint) -> Point {
        if let Some((id, db)) = self.pc_private(d) {
            if let Ok(k) = BoxedEcdsaPrivateKey::from_bytes(id, &db) {
                if let Some(p) = self.decode(&k.public_key().to_sec1()) {
                    return p;
                }
            }
        }
        self.mul(d, &self.generator())
    }

    /// Truncate a hash to the bit length of the order (ECDSA e value).
    fn hash_to_int(&self, hash: &[u8]) -> BigUint {
        let nbits = self.n.bits();
        let mut e = BigUint::from_bytes_be(hash);
        let hbits = hash.len() * 8;
        if hbits > nbits {
            e >>= hbits - nbits;
        }
        e
    }

    /// ECDSA sign, returning (r, s).
    pub fn ecdsa_sign(&self, d: &BigUint, hash: &[u8]) -> (BigUint, BigUint) {
        if let Some((id, db)) = self.pc_private(d) {
            if let Ok(k) = BoxedEcdsaPrivateKey::from_bytes(id, &db) {
                if let Ok(sig) = k.sign_prehash::<purecrypto::hash::Sha256>(hash) {
                    return (
                        BigUint::from_bytes_be(&sig.r_bytes(id)),
                        BigUint::from_bytes_be(&sig.s_bytes(id)),
                    );
                }
            }
        }
        let e = self.hash_to_int(hash);
        loop {
            let k = self.random_scalar();
            let r = match self.mul(&k, &self.generator()) {
                Point::Aff(x, _) => x % &self.n,
                Point::Inf => continue,
            };
            if r.is_zero() {
                continue;
            }
            let kinv = k.modpow(&(&self.n - BigUint::from(2u32)), &self.n);
            let s = (kinv * ((&e + &r * d) % &self.n)) % &self.n;
            if s.is_zero() {
                continue;
            }
            return (r, s);
        }
    }

    pub fn ecdsa_verify(&self, q: &Point, hash: &[u8], r: &BigUint, s: &BigUint) -> bool {
        if r.is_zero() || s.is_zero() || r >= &self.n || s >= &self.n {
            return false;
        }
        if let Some((_, k)) = self.pc_public(q) {
            let sig = BoxedEcdsaSignature::from_components(
                BoxedUint::from_be_bytes(&r.to_bytes_be()),
                BoxedUint::from_be_bytes(&s.to_bytes_be()),
            );
            return k.verify_prehash(hash, &sig).is_ok();
        }
        let e = self.hash_to_int(hash);
        let w = s.modpow(&(&self.n - BigUint::from(2u32)), &self.n);
        let u1 = (&e * &w) % &self.n;
        let u2 = (r * &w) % &self.n;
        let p1 = self.mul(&u1, &self.generator());
        let p2 = self.mul(&u2, q);
        match self.add(&p1, &p2) {
            Point::Aff(x, _) => &(x % &self.n) == r,
            Point::Inf => false,
        }
    }

    /// SM2 signature over a digest (no Z prefix, as OpenSSL 3 without an ID).
    pub fn sm2_sign(&self, d: &BigUint, hash: &[u8]) -> Option<(BigUint, BigUint)> {
        let n = &self.n;
        let e = BigUint::from_bytes_be(hash) % n;
        let one = BigUint::one();
        let inv = crate::pkey::mod_inv(&((d + &one) % n), n)?;
        loop {
            let k = self.random_scalar();
            let x1 = match self.mul(&k, &self.generator()) {
                Point::Aff(x, _) => x,
                Point::Inf => continue,
            };
            let r = (&e + x1) % n;
            if r.is_zero() || (&r + &k) == *n {
                continue;
            }
            let rd = (&r * d) % n;
            let kr = if k >= rd {
                &k - &rd
            } else {
                n - (&rd - &k) % n
            };
            let s = (&inv * kr) % n;
            if s.is_zero() {
                continue;
            }
            return Some((r, s));
        }
    }

    pub fn sm2_verify(&self, q: &Point, hash: &[u8], r: &BigUint, s: &BigUint) -> bool {
        let n = &self.n;
        if r.is_zero() || s.is_zero() || r >= n || s >= n {
            return false;
        }
        let e = BigUint::from_bytes_be(hash) % n;
        let t = (r + s) % n;
        if t.is_zero() {
            return false;
        }
        match self.add(&self.mul(s, &self.generator()), &self.mul(&t, q)) {
            Point::Aff(x1, _) => (e + x1) % n == *r,
            Point::Inf => false,
        }
    }

    /// ECDH shared secret: x coordinate of d*Q, padded to the field size.
    pub fn ecdh(&self, d: &BigUint, q: &Point) -> Option<Vec<u8>> {
        if let (Some((id, db)), Some((_, peer))) = (self.pc_private(d), self.pc_public(q)) {
            let k = BoxedEcdhPrivateKey::from_bytes(id, &db).ok()?;
            return k.diffie_hellman(&peer).ok();
        }
        match self.mul(d, q) {
            Point::Aff(x, _) => Some(pad(&x, self.field_len())),
            Point::Inf => None,
        }
    }

    /// Guess the cofactor (ec_guess_cofactor): h = (p + 1 + n/2) / n.
    pub fn guess_cofactor(&mut self) {
        if self.n.is_zero() {
            return;
        }
        self.h = (&self.p + BigUint::one() + (&self.n >> 1)) / &self.n;
    }
}

/// Uniform value in [1, n) from the OS RNG (rejection sampling).
pub fn random_below(n: &BigUint) -> BigUint {
    use purecrypto::rng::RngCore;
    let bits = n.bits();
    let len = bits.div_ceil(8);
    let mask = if bits.is_multiple_of(8) {
        0xff
    } else {
        (1u8 << (bits % 8)) - 1
    };
    loop {
        let mut b = vec![0u8; len];
        purecrypto::rng::OsRng.fill_bytes(&mut b);
        if let Some(f) = b.first_mut() {
            *f &= mask;
        }
        let k = BigUint::from_bytes_be(&b);
        if !k.is_zero() && &k < n {
            return k;
        }
    }
}

pub fn pad(x: &BigUint, len: usize) -> Vec<u8> {
    let b = x.to_bytes_be();
    if b.len() >= len {
        if b.len() > len && b.iter().take(b.len() - len).all(|&c| c == 0) {
            return b[b.len() - len..].to_vec();
        }
        return b;
    }
    let mut out = vec![0u8; len - b.len()];
    out.extend(b);
    out
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
