//! AES-GCM-SIV encryption (RFC 8452) as openssl_encrypt() exposes it:
//! the ciphertext without the tag (observed on PHP 8.5; PHP does not treat
//! the mode as AEAD, so AAD is never set and the tag never read).
//!
//! AES-128 and AES-256 run on purecrypto's implementation. OpenSSL also
//! lists AES-192-GCM-SIV, which RFC 8452 does not define; it is RFC 8452's
//! construction with the message-encryption key taken from three derived
//! blocks (24 bytes), the derivation of §4 extended in the obvious way
//! (checked against PHP 8.5's output). That variant uses the portable
//! POLYVAL below.

use purecrypto::cipher::{Aes192, AesGcmSiv, BlockCipher};
use purecrypto::zeroize::Zeroize;

use super::{Algo, CipherError};

pub(super) fn encrypt(
    algo: Algo,
    key: &[u8],
    iv: &[u8],
    data: &[u8],
) -> Result<Vec<u8>, CipherError> {
    if algo != Algo::Aes {
        return Err(CipherError::Unsupported);
    }
    let nonce: &[u8; 12] = iv.try_into().map_err(|_| CipherError::InvalidIvLength)?;
    let mut out = data.to_vec();
    if key.len() == 24 {
        let k = Aes192::new(key.try_into().expect("24 bytes"));
        let make = |ek: &[u8]| Aes192::new(ek.try_into().expect("24 bytes"));
        let _tag = seal(&k, 24, make, nonce, b"", &mut out);
        return Ok(out);
    }
    let g = AesGcmSiv::try_new(key).map_err(|_| CipherError::InvalidKeyLength)?;
    let _tag = g.encrypt(nonce, b"", &mut out);
    Ok(out)
}

/// POLYVAL multiplication (RFC 8452 §3): `a * b * x^-128` in GF(2^128)
/// modulo x^128 + x^127 + x^126 + x^121 + 1, elements little-endian.
/// Constant time (fixed iteration count, masks instead of branches).
fn dot(a: u128, b: u128) -> u128 {
    // r = a * b mod P.
    const R: u128 = (1 << 127) | (1 << 126) | (1 << 121) | 1;
    let mut r = 0u128;
    for i in (0..128).rev() {
        let carry = 0u128.wrapping_sub(r >> 127);
        r = (r << 1) ^ (carry & R);
        r ^= a & 0u128.wrapping_sub((b >> i) & 1);
    }
    // Multiply by x^-128: 128 divisions by x.
    for _ in 0..128 {
        let low = 0u128.wrapping_sub(r & 1);
        r = ((r ^ (low & R)) >> 1) ^ (low & (1 << 127));
    }
    r
}

fn polyval(h: u128, data: &[u8], acc: &mut u128) {
    for chunk in data.chunks(16) {
        let mut blk = [0u8; 16];
        blk[..chunk.len()].copy_from_slice(chunk);
        *acc = dot(*acc ^ u128::from_le_bytes(blk), h);
    }
}

/// RFC 8452 §4 encryption with a `key_len`-byte message-encryption key;
/// returns the tag.
fn seal<C: BlockCipher, K: BlockCipher>(
    kgk: &C,
    key_len: usize,
    make: impl FnOnce(&[u8]) -> K,
    nonce: &[u8; 12],
    aad: &[u8],
    buf: &mut [u8],
) -> [u8; 16] {
    // Key derivation: the low 8 bytes of AES(LE32(i) || nonce).
    let nblocks = 2 + key_len.div_ceil(8);
    let mut derived = vec![0u8; nblocks * 8];
    for i in 0..nblocks {
        let mut b = [0u8; 16];
        b[..4].copy_from_slice(&(i as u32).to_le_bytes());
        b[4..].copy_from_slice(nonce);
        kgk.encrypt_block(&mut b);
        derived[i * 8..i * 8 + 8].copy_from_slice(&b[..8]);
        b.zeroize();
    }
    let auth = u128::from_le_bytes(derived[..16].try_into().expect("16 bytes"));
    let enc = make(&derived[16..16 + key_len]);
    derived.zeroize();

    let mut s = 0u128;
    polyval(auth, aad, &mut s);
    polyval(auth, buf, &mut s);
    let mut lens = [0u8; 16];
    lens[..8].copy_from_slice(&((aad.len() as u64) * 8).to_le_bytes());
    lens[8..].copy_from_slice(&((buf.len() as u64) * 8).to_le_bytes());
    polyval(auth, &lens, &mut s);
    let mut sb = s.to_le_bytes();
    for (x, n) in sb.iter_mut().zip(nonce.iter()) {
        *x ^= *n;
    }
    sb[15] &= 0x7f;
    enc.encrypt_block(&mut sb);
    let tag = sb;

    // CTR from the tag with the top bit set, 32-bit little-endian counter.
    let mut ctr = tag;
    ctr[15] |= 0x80;
    let mut c = u32::from_le_bytes(ctr[..4].try_into().expect("4 bytes"));
    for chunk in buf.chunks_mut(16) {
        let mut ks = ctr;
        ks[..4].copy_from_slice(&c.to_le_bytes());
        enc.encrypt_block(&mut ks);
        for (b, k) in chunk.iter_mut().zip(ks.iter()) {
            *b ^= *k;
        }
        c = c.wrapping_add(1);
    }
    tag
}

#[cfg(test)]
mod tests {
    use super::*;
    use purecrypto::cipher::{Aes128, Aes256};

    fn h(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn polyval_rfc8452_appendix_a() {
        // RFC 8452 Appendix A: POLYVAL(H, X_1, X_2).
        let hk = u128::from_le_bytes(h("25629347589242761d31f826ba4b757b").try_into().unwrap());
        let x = h("4f4f95668c83dfb6401762bb2d01a262d1a24ddd2721d006bbe45f20d3c9f362");
        let mut s = 0;
        polyval(hk, &x, &mut s);
        assert_eq!(
            s.to_le_bytes().to_vec(),
            h("f7a3b47b846119fae5b7866cf5e5b77e")
        );
    }

    #[test]
    fn generic_seal_matches_purecrypto() {
        // The generic construction with AES-128 and AES-256 against
        // purecrypto's RFC 8452 implementation.
        for (klen, n) in [(16usize, 0usize), (16, 1), (16, 37), (32, 64), (32, 5)] {
            let key: Vec<u8> = (0..klen as u8).map(|i| i.wrapping_mul(13) ^ 0x5a).collect();
            let nonce = [7u8; 12];
            let aad = b"some aad";
            let pt: Vec<u8> = (0..n as u8).collect();
            let mut a = pt.clone();
            let ta = AesGcmSiv::new(&key).encrypt(&nonce, aad, &mut a);
            let mut b = pt.clone();
            let tb = if klen == 16 {
                let k = Aes128::new(key[..].try_into().unwrap());
                seal(
                    &k,
                    16,
                    |e| Aes128::new(e.try_into().unwrap()),
                    &nonce,
                    aad,
                    &mut b,
                )
            } else {
                let k = Aes256::new(key[..].try_into().unwrap());
                seal(
                    &k,
                    32,
                    |e| Aes256::new(e.try_into().unwrap()),
                    &nonce,
                    aad,
                    &mut b,
                )
            };
            assert_eq!(a, b);
            assert_eq!(ta, tb);
        }
    }
}
