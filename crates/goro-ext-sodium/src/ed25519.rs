//! libsodium's crypto_sign_ed25519 (ref10) over purecrypto's edwards25519
//! group: key pairs whose secret key is seed || public key (the signature
//! uses the public half of the secret key as given), signatures, the strict
//! verification checks (canonical S, non-small-order R and A, canonical
//! A), and the Ed25519 -> X25519 key conversions.

use purecrypto::ec::edwards25519::hazmat::{EdwardsPoint, Scalar};
use purecrypto::hash::{Digest, Sha512};

/// p = 2^255 - 19, little-endian.
const P: [u8; 32] = {
    let mut p = [0xffu8; 32];
    p[0] = 0xed;
    p[31] = 0x7f;
    p
};

/// The y coordinate of a point of order 8 (the other one is p - this).
const ORDER8_Y: [u8; 32] = [
    0x26, 0xe8, 0x95, 0x8f, 0xc2, 0xb2, 0x27, 0xb0, 0x45, 0xc3, 0xf4, 0x89, 0xf2, 0xef, 0x98, 0xf0,
    0xd5, 0xdf, 0xac, 0x05, 0xd3, 0xc6, 0x33, 0x39, 0xb1, 0x38, 0x02, 0x88, 0x6d, 0x53, 0xfc, 0x05,
];

fn sub_le(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let mut out = *a;
    crate::codecs::sub(&mut out, b);
    out
}

fn small_int(n: u8) -> [u8; 32] {
    let mut v = [0u8; 32];
    v[0] = n;
    v
}

/// ge25519_has_small_order(): the encoding (sign bit ignored) is one of the
/// seven blacklisted small-order y coordinates (0, 1, the two of order 8,
/// p - 1, p, p + 1).
pub fn has_small_order(s: &[u8; 32]) -> bool {
    let mut t = *s;
    t[31] &= 0x7f;
    let mut p_plus_1 = P;
    p_plus_1[0] = 0xee;
    let list = [
        small_int(0),
        small_int(1),
        ORDER8_Y,
        sub_le(&P, &ORDER8_Y),
        sub_le(&P, &small_int(1)),
        P,
        p_plus_1,
    ];
    let mut found = 0u8;
    for e in &list {
        let d = e.iter().zip(t.iter()).fold(0u8, |a, (x, y)| a | (x ^ y));
        found |= (d == 0) as u8;
    }
    found != 0
}

/// ge25519_is_canonical(): y < p.
pub fn is_canonical(s: &[u8; 32]) -> bool {
    let mut c = (s[31] & 0x7f) ^ 0x7f;
    for &b in &s[1..31] {
        c |= b ^ 0xff;
    }
    !(c == 0 && s[0] >= 0xed)
}

fn sha512_parts(parts: &[&[u8]]) -> [u8; 64] {
    let mut h = Sha512::new();
    for p in parts {
        h.update(p);
    }
    h.finalize()
}

fn reduce64(b: &[u8; 64]) -> Scalar {
    Scalar::from_bytes_mod_order(b)
}

fn scalar32(b: &[u8]) -> Scalar {
    let mut w = [0u8; 64];
    w[..32].copy_from_slice(&b[..32]);
    let s = Scalar::from_bytes_mod_order(&w);
    w.fill(0);
    s
}

fn clamp(az: &mut [u8]) {
    az[0] &= 248;
    az[31] &= 127;
    az[31] |= 64;
}

/// crypto_sign_seed_keypair(): (pk, sk = seed || pk).
pub fn seed_keypair(seed: &[u8; 32]) -> ([u8; 32], [u8; 64]) {
    let mut az = sha512_parts(&[seed]);
    clamp(&mut az);
    let a = scalar32(&az);
    az.fill(0);
    let pk = EdwardsPoint::mul_base(&a).compress();
    let mut sk = [0u8; 64];
    sk[..32].copy_from_slice(seed);
    sk[32..].copy_from_slice(&pk);
    (pk, sk)
}

/// crypto_sign_detached()
pub fn sign_detached(m: &[u8], sk: &[u8; 64]) -> [u8; 64] {
    let mut az = sha512_parts(&[&sk[..32]]);
    let mut nonce = sha512_parts(&[&az[32..], m]);
    let r = reduce64(&nonce);
    nonce.fill(0);
    let rb = EdwardsPoint::mul_base(&r).compress();
    let hram = reduce64(&sha512_parts(&[&rb, &sk[32..], m]));
    clamp(&mut az);
    let a = scalar32(&az);
    az.fill(0);
    let s = hram.mul(&a).add(&r);
    let mut sig = [0u8; 64];
    sig[..32].copy_from_slice(&rb);
    sig[32..].copy_from_slice(&s.to_bytes());
    sig
}

/// crypto_sign_verify_detached()
pub fn verify_detached(sig: &[u8; 64], m: &[u8], pk: &[u8; 32]) -> bool {
    let r: [u8; 32] = sig[..32].try_into().unwrap();
    let Some(s) = Scalar::from_bytes_canonical(sig[32..].try_into().unwrap()) else {
        return false;
    };
    if has_small_order(&r) || !is_canonical(pk) || has_small_order(pk) {
        return false;
    }
    let Some(a) = EdwardsPoint::decompress(pk) else {
        return false;
    };
    let h = reduce64(&sha512_parts(&[&r, pk, m]));
    let check = EdwardsPoint::mul_base(&s).sub(&a.mul(&h)).compress();
    crate::codecs::memcmp(&check, &r) == 0
}

/// crypto_sign_ed25519_sk_to_curve25519()
pub fn sk_to_curve25519(sk: &[u8; 64]) -> [u8; 32] {
    let mut h = sha512_parts(&[&sk[..32]]);
    clamp(&mut h);
    let mut out = [0u8; 32];
    out.copy_from_slice(&h[..32]);
    h.fill(0);
    out
}

/// crypto_sign_ed25519_pk_to_curve25519(): u = (1 + y) / (1 - y), for a
/// point of the prime-order subgroup.
pub fn pk_to_curve25519(pk: &[u8; 32]) -> Option<[u8; 32]> {
    if has_small_order(pk) {
        return None;
    }
    // ge25519_frombytes() reduces a non-canonical y modulo p.
    let mut enc = *pk;
    if !is_canonical(&enc) {
        let sign = enc[31] & 0x80;
        enc[31] &= 0x7f;
        enc = sub_le(&enc, &P);
        enc[31] |= sign;
    }
    let a = EdwardsPoint::decompress(&enc)?;
    if !bool::from(a.is_torsion_free()) {
        return None;
    }
    let y = Fe::from_bytes(&a.y_bytes());
    let one = Fe::ONE;
    let u = one.add(&y).mul(&one.sub(&y).invert());
    Some(u.to_bytes())
}

/// Minimal GF(2^255 - 19) arithmetic (radix 2^51) for the public-key
/// conversion; purecrypto keeps its field type private.
#[derive(Clone, Copy)]
struct Fe([u64; 5]);

const MASK51: u64 = (1 << 51) - 1;

impl Fe {
    const ONE: Fe = Fe([1, 0, 0, 0, 0]);

    fn from_bytes(b: &[u8; 32]) -> Fe {
        let mut w = [0u64; 4];
        for (i, x) in w.iter_mut().enumerate() {
            *x = u64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap());
        }
        Fe([
            w[0] & MASK51,
            ((w[0] >> 51) | (w[1] << 13)) & MASK51,
            ((w[1] >> 38) | (w[2] << 26)) & MASK51,
            ((w[2] >> 25) | (w[3] << 39)) & MASK51,
            (w[3] >> 12) & MASK51,
        ])
    }

    fn carry(mut l: [u128; 5]) -> Fe {
        for _ in 0..2 {
            for i in 0..4 {
                l[i + 1] += l[i] >> 51;
                l[i] &= MASK51 as u128;
            }
            let c = l[4] >> 51;
            l[4] &= MASK51 as u128;
            l[0] += c * 19;
        }
        Fe([
            l[0] as u64,
            l[1] as u64,
            l[2] as u64,
            l[3] as u64,
            l[4] as u64,
        ])
    }

    fn add(&self, o: &Fe) -> Fe {
        let mut l = [0u128; 5];
        for (i, x) in l.iter_mut().enumerate() {
            *x = self.0[i] as u128 + o.0[i] as u128;
        }
        Fe::carry(l)
    }

    fn sub(&self, o: &Fe) -> Fe {
        // + 4p to stay positive
        const FOUR_P: [u64; 5] = [
            4 * ((1 << 51) - 19),
            4 * MASK51,
            4 * MASK51,
            4 * MASK51,
            4 * MASK51,
        ];
        let mut l = [0u128; 5];
        for (i, x) in l.iter_mut().enumerate() {
            *x = self.0[i] as u128 + FOUR_P[i] as u128 - o.0[i] as u128;
        }
        Fe::carry(l)
    }

    fn mul(&self, o: &Fe) -> Fe {
        let a = self.0.map(|x| x as u128);
        let b = o.0.map(|x| x as u128);
        let mut l = [0u128; 5];
        for i in 0..5 {
            for j in 0..5 {
                let p = a[i] * b[j];
                if i + j < 5 {
                    l[i + j] += p;
                } else {
                    l[i + j - 5] += p * 19;
                }
            }
        }
        Fe::carry(l)
    }

    fn invert(&self) -> Fe {
        // self^(p - 2), p - 2 = 2^255 - 21
        let mut e = P;
        e[0] = 0xeb;
        let mut r = Fe::ONE;
        for i in (0..255).rev() {
            r = r.mul(&r);
            if (e[i / 8] >> (i % 8)) & 1 == 1 {
                r = r.mul(self);
            }
        }
        r
    }

    fn to_bytes(self) -> [u8; 32] {
        let l = Fe::carry(self.0.map(|x| x as u128)).0;
        // The (small multiple of p) value as a 320-bit integer, then folded
        // below 2^255 and reduced into [0, p).
        let mut w = [0u64; 5];
        let mut acc: u128 = 0;
        let mut bits = 0u32;
        let mut wi = 0;
        for &x in &l {
            acc += (x as u128) << bits;
            bits += 51;
            while bits >= 64 {
                w[wi] = acc as u64;
                wi += 1;
                acc >>= 64;
                bits -= 64;
            }
        }
        w[wi] = acc as u64;
        for _ in 0..2 {
            let hi = (w[3] >> 63) | (w[4] << 1);
            w[3] &= (1 << 63) - 1;
            w[4] = 0;
            let mut c = hi as u128 * 19;
            for x in w.iter_mut().take(4) {
                c += *x as u128;
                *x = c as u64;
                c >>= 64;
            }
            w[4] = c as u64;
        }
        let mut out = [0u8; 32];
        for (i, x) in w.iter().take(4).enumerate() {
            out[i * 8..i * 8 + 8].copy_from_slice(&x.to_le_bytes());
        }
        if !is_canonical(&out) {
            out = sub_le(&out, &P);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn rfc8032_test1() {
        let seed = hex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60");
        let (pk, sk) = seed_keypair(seed[..].try_into().unwrap());
        assert_eq!(
            pk.to_vec(),
            hex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
        );
        let sig = sign_detached(b"", &sk);
        assert_eq!(
            sig.to_vec(),
            hex(
                "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155\
                 5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
            )
        );
        assert!(verify_detached(&sig, b"", &pk));
        assert!(!verify_detached(&sig, b"x", &pk));
    }

    #[test]
    fn order8_is_small() {
        let p = EdwardsPoint::decompress(&ORDER8_Y).unwrap();
        assert!(bool::from(p.is_small_order()));
        let q = EdwardsPoint::decompress(&sub_le(&P, &ORDER8_Y)).unwrap();
        assert!(bool::from(q.is_small_order()));
    }

    #[test]
    fn pk_conversion_matches_x25519() {
        let seed = [42u8; 32];
        let (pk, sk) = seed_keypair(&seed);
        let xsk = sk_to_curve25519(&sk);
        let xpk = pk_to_curve25519(&pk).unwrap();
        let base = purecrypto::ec::x25519::BASE_POINT;
        assert_eq!(purecrypto::ec::x25519::x25519(&xsk, &base), xpk);
    }
}
