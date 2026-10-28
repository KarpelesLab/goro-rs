//! Arithmetic in GF(2^255 − 19) and the X25519 Montgomery ladder
//! (RFC 7748 §5).
//!
//! Elements are five 51-bit limbs in `u64`s; products use `u128`. Every
//! operation is branch-free and table-free, so it is constant-time; the
//! ladder swaps with masks. Used for X25519 (purecrypto's ladder is slower
//! than libsodium's on the reference machine), for the Edwards-to-Montgomery
//! map `u = (1 + y) / (1 − y)` (RFC 7748 §4.1) and for the variable-time
//! verification arithmetic in [`crate::ed25519`].

const MASK: u64 = (1 << 51) - 1;

/// A field element (limbs below 2^52 between operations).
#[derive(Clone, Copy, Debug)]
pub struct Fe(pub [u64; 5]);

#[inline(always)]
fn m(a: u64, b: u64) -> u128 {
    a as u128 * b as u128
}

#[inline(always)]
fn load8(b: &[u8]) -> u64 {
    u64::from_le_bytes(b[..8].try_into().unwrap())
}

impl Fe {
    pub const ZERO: Fe = Fe([0; 5]);
    pub const ONE: Fe = Fe([1, 0, 0, 0, 0]);

    /// Reads 32 little-endian bytes, ignoring the top bit (RFC 7748 §5:
    /// non-canonical values are accepted and reduced).
    #[inline]
    pub fn from_bytes(b: &[u8; 32]) -> Fe {
        Fe([
            load8(&b[0..]) & MASK,
            (load8(&b[6..]) >> 3) & MASK,
            (load8(&b[12..]) >> 6) & MASK,
            (load8(&b[19..]) >> 1) & MASK,
            (load8(&b[24..]) >> 12) & MASK,
        ])
    }

    /// The canonical 32-byte encoding.
    pub fn to_bytes(self) -> [u8; 32] {
        let mut h = self.carry().0;
        // h < 2^255 + small: subtract p once if h >= p (q is 0 or 1).
        let mut q = (h[0] + 19) >> 51;
        q = (h[1] + q) >> 51;
        q = (h[2] + q) >> 51;
        q = (h[3] + q) >> 51;
        q = (h[4] + q) >> 51;
        h[0] += 19 * q;
        h[1] += h[0] >> 51;
        h[0] &= MASK;
        h[2] += h[1] >> 51;
        h[1] &= MASK;
        h[3] += h[2] >> 51;
        h[2] &= MASK;
        h[4] += h[3] >> 51;
        h[3] &= MASK;
        h[4] &= MASK;
        let w0 = h[0] | (h[1] << 51);
        let w1 = (h[1] >> 13) | (h[2] << 38);
        let w2 = (h[2] >> 26) | (h[3] << 25);
        let w3 = (h[3] >> 39) | (h[4] << 12);
        let mut out = [0u8; 32];
        out[0..8].copy_from_slice(&w0.to_le_bytes());
        out[8..16].copy_from_slice(&w1.to_le_bytes());
        out[16..24].copy_from_slice(&w2.to_le_bytes());
        out[24..32].copy_from_slice(&w3.to_le_bytes());
        out
    }

    /// One carry pass: limbs below 2^51 + 2^13·19.
    #[inline(always)]
    fn carry(self) -> Fe {
        let mut h = self.0;
        let c0 = h[0] >> 51;
        h[0] &= MASK;
        h[1] += c0;
        let c1 = h[1] >> 51;
        h[1] &= MASK;
        h[2] += c1;
        let c2 = h[2] >> 51;
        h[2] &= MASK;
        h[3] += c2;
        let c3 = h[3] >> 51;
        h[3] &= MASK;
        h[4] += c3;
        let c4 = h[4] >> 51;
        h[4] &= MASK;
        h[0] += c4 * 19;
        Fe(h)
    }

    #[inline(always)]
    pub fn add(self, b: Fe) -> Fe {
        let a = self.0;
        let b = b.0;
        Fe([
            a[0] + b[0],
            a[1] + b[1],
            a[2] + b[2],
            a[3] + b[3],
            a[4] + b[4],
        ])
    }

    /// `self − b` (inputs with limbs below 2^54).
    #[inline(always)]
    pub fn sub(self, b: Fe) -> Fe {
        // Add 16p limb-wise so nothing underflows, then carry.
        let a = self.0;
        let b = b.0;
        Fe([
            (a[0] + 0x7ffffffffffed0) - b[0],
            (a[1] + 0x7ffffffffffff0) - b[1],
            (a[2] + 0x7ffffffffffff0) - b[2],
            (a[3] + 0x7ffffffffffff0) - b[3],
            (a[4] + 0x7ffffffffffff0) - b[4],
        ])
        .carry()
    }

    /// `self − b` for `b` with limbs below 2^53 (a sum of two products),
    /// without the carry pass: limbs stay below 2^54 when `self`'s are
    /// below 2^53.
    #[inline(always)]
    pub fn sub_lazy4(self, b: Fe) -> Fe {
        let a = self.0;
        let b = b.0;
        Fe([
            (a[0] + 0x1fffffffffffb4) - b[0],
            (a[1] + 0x1ffffffffffffc) - b[1],
            (a[2] + 0x1ffffffffffffc) - b[2],
            (a[3] + 0x1ffffffffffffc) - b[3],
            (a[4] + 0x1ffffffffffffc) - b[4],
        ])
    }

    /// `self − b` for `b` with limbs below 2^52 (a product or square),
    /// without the carry pass: limbs stay below 2^53.
    #[inline(always)]
    pub fn sub_lazy(self, b: Fe) -> Fe {
        let a = self.0;
        let b = b.0;
        Fe([
            (a[0] + 0xfffffffffffda) - b[0],
            (a[1] + 0xffffffffffffe) - b[1],
            (a[2] + 0xffffffffffffe) - b[2],
            (a[3] + 0xffffffffffffe) - b[3],
            (a[4] + 0xffffffffffffe) - b[4],
        ])
    }

    #[inline(always)]
    pub fn neg(self) -> Fe {
        Fe::ZERO.sub(self)
    }

    #[inline(always)]
    fn reduce_wide(c: [u128; 5]) -> Fe {
        let mut c = c;
        let mut out = [0u64; 5];
        c[1] += c[0] >> 51;
        out[0] = c[0] as u64 & MASK;
        c[2] += c[1] >> 51;
        out[1] = c[1] as u64 & MASK;
        c[3] += c[2] >> 51;
        out[2] = c[2] as u64 & MASK;
        c[4] += c[3] >> 51;
        out[3] = c[3] as u64 & MASK;
        let carry = (c[4] >> 51) as u64;
        out[4] = c[4] as u64 & MASK;
        out[0] += carry * 19;
        out[1] += out[0] >> 51;
        out[0] &= MASK;
        Fe(out)
    }

    #[inline(always)]
    pub fn mul(self, b: Fe) -> Fe {
        let a = self.0;
        let b = b.0;
        let b1 = b[1] * 19;
        let b2 = b[2] * 19;
        let b3 = b[3] * 19;
        let b4 = b[4] * 19;
        Fe::reduce_wide([
            m(a[0], b[0]) + m(a[1], b4) + m(a[2], b3) + m(a[3], b2) + m(a[4], b1),
            m(a[0], b[1]) + m(a[1], b[0]) + m(a[2], b4) + m(a[3], b3) + m(a[4], b2),
            m(a[0], b[2]) + m(a[1], b[1]) + m(a[2], b[0]) + m(a[3], b4) + m(a[4], b3),
            m(a[0], b[3]) + m(a[1], b[2]) + m(a[2], b[1]) + m(a[3], b[0]) + m(a[4], b4),
            m(a[0], b[4]) + m(a[1], b[3]) + m(a[2], b[2]) + m(a[3], b[1]) + m(a[4], b[0]),
        ])
    }

    #[inline(always)]
    pub fn square(self) -> Fe {
        let a = self.0;
        let d0 = a[0] * 2;
        let d1 = a[1] * 2;
        let d2 = a[2] * 2 * 19;
        let d3 = a[3] * 19;
        let d4 = a[4] * 19;
        Fe::reduce_wide([
            m(a[0], a[0]) + m(d1, d4) + m(d2, a[3]),
            m(d0, a[1]) + m(d2, a[4]) + m(d3, a[3]),
            m(d0, a[2]) + m(a[1], a[1]) + m(2 * a[3], d4),
            m(d0, a[3]) + m(d1, a[2]) + m(a[4], d4),
            m(d0, a[4]) + m(d1, a[3]) + m(a[2], a[2]),
        ])
    }

    #[inline(always)]
    fn sqn(self, n: u32) -> Fe {
        let mut x = self;
        for _ in 0..n {
            x = x.square();
        }
        x
    }

    /// `self · 121665` (the RFC 7748 constant a24).
    #[inline(always)]
    fn mul_a24(self) -> Fe {
        let a = self.0;
        Fe::reduce_wide([
            m(a[0], 121665),
            m(a[1], 121665),
            m(a[2], 121665),
            m(a[3], 121665),
            m(a[4], 121665),
        ])
    }

    /// `self^(2^250 − 1)` and `self^11`, shared by inversion and square roots.
    #[inline(always)]
    fn pow22501(self) -> (Fe, Fe) {
        let z2 = self.square();
        let z9 = z2.sqn(2).mul(self);
        let z11 = z9.mul(z2);
        let z_5_0 = z11.square().mul(z9);
        let z_10_0 = z_5_0.sqn(5).mul(z_5_0);
        let z_20_0 = z_10_0.sqn(10).mul(z_10_0);
        let z_40_0 = z_20_0.sqn(20).mul(z_20_0);
        let z_50_0 = z_40_0.sqn(10).mul(z_10_0);
        let z_100_0 = z_50_0.sqn(50).mul(z_50_0);
        let z_200_0 = z_100_0.sqn(100).mul(z_100_0);
        let z_250_0 = z_200_0.sqn(50).mul(z_50_0);
        (z_250_0, z11)
    }

    /// `1 / self` (`self^(p − 2)`); zero maps to zero.
    #[inline(always)]
    pub fn invert(self) -> Fe {
        let (t, z11) = self.pow22501();
        t.sqn(5).mul(z11)
    }

    /// `self^((p − 5) / 8)`, for square roots.
    #[inline(always)]
    pub fn pow_p58(self) -> Fe {
        let (t, _) = self.pow22501();
        t.sqn(2).mul(self)
    }

    /// Whether the element is zero (constant-time).
    pub fn is_zero(self) -> bool {
        crate::ct::is_zero(&self.to_bytes())
    }

    /// The low bit of the canonical encoding ("negative" in RFC 8032).
    pub fn is_negative(self) -> u8 {
        self.to_bytes()[0] & 1
    }

    #[cfg(test)]
    pub fn ct_eq(self, b: Fe) -> bool {
        crate::ct::eq(&self.to_bytes(), &b.to_bytes())
    }

    /// Swaps `a` and `b` when `mask` is all ones (constant-time).
    #[inline(always)]
    fn cswap(a: &mut Fe, b: &mut Fe, mask: u64) {
        for i in 0..5 {
            let t = mask & (a.0[i] ^ b.0[i]);
            a.0[i] ^= t;
            b.0[i] ^= t;
        }
    }
}

/// Whether the CPU has BMI2 (`mulx`), used by the hot paths here: with it
/// the 64×64→128-bit products are not tied to `rax`/`rdx`.
#[cfg(target_arch = "x86_64")]
#[inline]
pub(crate) fn has_bmi2() -> bool {
    std::is_x86_feature_detected!("bmi2")
}

/// X25519 (RFC 7748 §5): `scalar` is clamped, the top bit of `u` ignored.
pub fn x25519(scalar: &[u8; 32], u: &[u8; 32]) -> [u8; 32] {
    #[cfg(target_arch = "x86_64")]
    if has_bmi2() {
        // SAFETY: BMI2 is available.
        return unsafe { x25519_bmi2(scalar, u) };
    }
    x25519_impl(scalar, u)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "bmi2")]
unsafe fn x25519_bmi2(scalar: &[u8; 32], u: &[u8; 32]) -> [u8; 32] {
    x25519_impl(scalar, u)
}

#[inline(always)]
fn x25519_impl(scalar: &[u8; 32], u: &[u8; 32]) -> [u8; 32] {
    let mut k = *scalar;
    k[0] &= 248;
    k[31] &= 127;
    k[31] |= 64;
    let x1 = Fe::from_bytes(u);
    let mut x2 = Fe::ONE;
    let mut z2 = Fe::ZERO;
    let mut x3 = x1;
    let mut z3 = Fe::ONE;
    let mut swap = 0u64;
    for t in (0..255).rev() {
        let bit = ((k[t >> 3] >> (t & 7)) & 1) as u64;
        swap ^= bit;
        let mask = swap.wrapping_neg();
        Fe::cswap(&mut x2, &mut x3, mask);
        Fe::cswap(&mut z2, &mut z3, mask);
        swap = bit;
        let a = x2.add(z2);
        let aa = a.square();
        let b = x2.sub_lazy(z2);
        let bb = b.square();
        let e = aa.sub_lazy(bb);
        let c = x3.add(z3);
        let d = x3.sub_lazy(z3);
        let da = d.mul(a);
        let cb = c.mul(b);
        x3 = da.add(cb).square();
        z3 = x1.mul(da.sub_lazy(cb).square());
        x2 = aa.mul(bb);
        z2 = e.mul(aa.add(e.mul_a24()));
    }
    let mask = swap.wrapping_neg();
    Fe::cswap(&mut x2, &mut x3, mask);
    Fe::cswap(&mut z2, &mut z3, mask);
    crate::ct::wipe(&mut k);
    x2.mul(z2.invert()).to_bytes()
}

/// The Montgomery `u` of the Edwards point with affine `y`:
/// `u = (1 + y) / (1 − y)` (RFC 7748 §4.1).
pub fn edwards_y_to_montgomery_u(y: &[u8; 32]) -> [u8; 32] {
    let y = Fe::from_bytes(y);
    Fe::ONE.add(y).mul(Fe::ONE.sub(y).invert()).to_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(s: &str) -> [u8; 32] {
        let v: Vec<u8> = (0..32)
            .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
            .collect();
        v.try_into().unwrap()
    }

    #[test]
    fn rfc7748_vectors() {
        // RFC 7748 §5.2.
        assert_eq!(
            x25519(
                &h("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4"),
                &h("e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c")
            ),
            h("c3da55379de9c6908e94ea4df28d084f32eccf03491c71f754b4075577a28552")
        );
        assert_eq!(
            x25519(
                &h("4b66e9d4d1b4673c5ad22691957d6af5c11b6421e0ea01d42ca4169e7918ba0d"),
                &h("e5210f12786811d3f4b7959d0538ae2c31dbe7106fc03c3efc4cd549c715a493")
            ),
            h("95cbde9476e8907d7aade45cb4b873f88b595a68799fa152e6f8f7647aac7957")
        );
        // §6.1: Alice's public key and the shared secret.
        let alice = h("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
        let bob_pub = h("de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f");
        let mut nine = [0u8; 32];
        nine[0] = 9;
        assert_eq!(
            x25519(&alice, &nine),
            h("8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a")
        );
        assert_eq!(
            x25519(&alice, &bob_pub),
            h("4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742")
        );
    }

    #[test]
    #[ignore]
    fn bench_ops() {
        let mut x = Fe::from_bytes(&h(
            "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20",
        ));
        let y = Fe::from_bytes(&h(
            "1102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f2f",
        ));
        let n = 1_000_000;
        let t = std::time::Instant::now();
        for _ in 0..n {
            x = x.mul(y);
        }
        eprintln!(
            "mul: {:.2} ns {:?}",
            t.elapsed().as_nanos() as f64 / n as f64,
            x.to_bytes()[0]
        );
        let t = std::time::Instant::now();
        for _ in 0..n {
            x = x.square();
        }
        eprintln!(
            "square: {:.2} ns {:?}",
            t.elapsed().as_nanos() as f64 / n as f64,
            x.to_bytes()[0]
        );
        let t = std::time::Instant::now();
        for _ in 0..1000 {
            x = Fe::from_bytes(&x25519(&x.to_bytes(), &y.to_bytes()));
        }
        eprintln!(
            "x25519: {:.2} us",
            t.elapsed().as_nanos() as f64 / 1000.0 / 1000.0
        );
        eprintln!(
            "avx512f {} ifma {} bmi2 {} adx {}",
            std::is_x86_feature_detected!("avx512f"),
            std::is_x86_feature_detected!("avx512ifma"),
            std::is_x86_feature_detected!("bmi2"),
            std::is_x86_feature_detected!("adx")
        );
    }

    #[test]
    fn canonical_encoding() {
        // p itself encodes as zero, p + 1 as one.
        let p = h("edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f");
        assert_eq!(Fe::from_bytes(&p).to_bytes(), [0; 32]);
        let p1 = h("eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f");
        let mut one = [0u8; 32];
        one[0] = 1;
        assert_eq!(Fe::from_bytes(&p1).to_bytes(), one);
        let x = Fe::from_bytes(&h(
            "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20",
        ));
        assert!(x.mul(x.invert()).ct_eq(Fe::ONE));
        assert!(x.square().ct_eq(x.mul(x)));
    }
}
