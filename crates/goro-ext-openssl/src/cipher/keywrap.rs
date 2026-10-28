//! Key wrapping: AES key wrap (RFC 3394) and key wrap with padding
//! (RFC 5649), with the inverse-cipher variants of NIST SP 800-38F §5.1
//! (the forward and inverse cipher functions swapped), and the CMS
//! Triple-DES key wrap (RFC 3217 §3).
//!
//! OpenSSL's `aes-*-wrap` take an explicit 8-byte initial value as the IV
//! and `aes-*-wrap-pad` a 4-byte one (the high half of the alternative
//! initial value, followed by the 32-bit message length indicator);
//! openssl_encrypt() without an IV pads it with zeros (observed on PHP 8.5:
//! the result equals that of an all-zero IV, not the RFC's default value).
//! An empty input wraps to an empty output (observed on PHP 8.5).

use purecrypto::ct::ConstantTimeEq;
use purecrypto::zeroize::Zeroize;

use super::{Bc, CipherError, Mode};

/// RFC 3394 §2.2.1 W(A, R) with `forward` choosing CIPH or CIPH^-1.
fn w<B: Bc>(c: &B, a: &mut [u8; 8], r: &mut [u8], forward: bool) {
    let n = r.len() / 8;
    let mut b = [0u8; 16];
    for j in 0..6u64 {
        for i in 0..n {
            b[..8].copy_from_slice(a);
            b[8..].copy_from_slice(&r[i * 8..i * 8 + 8]);
            if forward {
                c.e(&mut b);
            } else {
                c.d(&mut b);
            }
            let t = (n as u64) * j + i as u64 + 1;
            a.copy_from_slice(&b[..8]);
            for (x, y) in a.iter_mut().zip(t.to_be_bytes()) {
                *x ^= y;
            }
            r[i * 8..i * 8 + 8].copy_from_slice(&b[8..]);
        }
    }
    b.zeroize();
}

/// RFC 3394 §2.2.2 W^-1(C), `forward` being the cipher direction of the
/// matching W.
fn w_inv<B: Bc>(c: &B, a: &mut [u8; 8], r: &mut [u8], forward: bool) {
    let n = r.len() / 8;
    let mut b = [0u8; 16];
    for j in (0..6u64).rev() {
        for i in (0..n).rev() {
            let t = (n as u64) * j + i as u64 + 1;
            for (x, y) in a.iter_mut().zip(t.to_be_bytes()) {
                *x ^= y;
            }
            b[..8].copy_from_slice(a);
            b[8..].copy_from_slice(&r[i * 8..i * 8 + 8]);
            if forward {
                c.d(&mut b);
            } else {
                c.e(&mut b);
            }
            a.copy_from_slice(&b[..8]);
            r[i * 8..i * 8 + 8].copy_from_slice(&b[8..]);
        }
    }
    b.zeroize();
}

pub(super) fn wrap<B: Bc>(
    c: &B,
    mode: Mode,
    iv: &[u8],
    data: &[u8],
) -> Result<Vec<u8>, CipherError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let forward = matches!(mode, Mode::Wrap | Mode::WrapPad);
    let mut a = [0u8; 8];
    match mode {
        Mode::Wrap | Mode::WrapInv => {
            if !data.len().is_multiple_of(8) {
                return Err(CipherError::KwInvalidLength);
            }
            if data.len() < 16 {
                return Err(CipherError::KwFailed);
            }
            a.copy_from_slice(iv);
            let mut out = vec![0u8; 8 + data.len()];
            out[8..].copy_from_slice(data);
            w(c, &mut a, &mut out[8..], forward);
            out[..8].copy_from_slice(&a);
            Ok(out)
        }
        _ => {
            // RFC 5649 §4.1: AIV = IV || MLI, the plaintext zero-padded to
            // a multiple of 8; a single padded block is enciphered as one
            // 128-bit block.
            a[..4].copy_from_slice(iv);
            a[4..].copy_from_slice(&(data.len() as u32).to_be_bytes());
            let padded = data.len().div_ceil(8) * 8;
            let mut out = vec![0u8; 8 + padded];
            out[8..8 + data.len()].copy_from_slice(data);
            if padded == 8 {
                out[..8].copy_from_slice(&a);
                if forward {
                    c.e(&mut out);
                } else {
                    c.d(&mut out);
                }
            } else {
                w(c, &mut a, &mut out[8..], forward);
                out[..8].copy_from_slice(&a);
            }
            Ok(out)
        }
    }
}

pub(super) fn unwrap<B: Bc>(
    c: &B,
    mode: Mode,
    iv: &[u8],
    data: &[u8],
) -> Result<Vec<u8>, CipherError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    if !data.len().is_multiple_of(8) || data.len() < 16 {
        return Err(CipherError::KwInvalidLength);
    }
    let forward = matches!(mode, Mode::Wrap | Mode::WrapPad);
    let mut a: [u8; 8] = data[..8].try_into().expect("8 bytes");
    let mut r = data[8..].to_vec();
    match mode {
        Mode::Wrap | Mode::WrapInv => {
            if data.len() < 24 {
                return Err(CipherError::KwFailed);
            }
            w_inv(c, &mut a, &mut r, forward);
            // The verdict is the public outcome of unwrapping.
            if !bool::from(a[..].ct_eq(iv)) {
                r.zeroize();
                return Err(CipherError::KwFailed);
            }
            Ok(r)
        }
        _ => {
            if data.len() == 16 {
                let mut b = [0u8; 16];
                b.copy_from_slice(data);
                if forward {
                    c.d(&mut b);
                } else {
                    c.e(&mut b);
                }
                a.copy_from_slice(&b[..8]);
                r.copy_from_slice(&b[8..]);
                b.zeroize();
            } else {
                w_inv(c, &mut a, &mut r, forward);
            }
            // RFC 5649 §4.2 checks: the AIV's high half, 8(n-1) < MLI <= 8n
            // and zero padding.
            let n = r.len();
            let mli = u32::from_be_bytes(a[4..].try_into().expect("4 bytes")) as usize;
            let mut ok = bool::from(a[..4].ct_eq(iv)) && mli <= n && mli + 8 > n;
            if ok {
                let zeros = [0u8; 8];
                ok = bool::from(r[mli..].ct_eq(&zeros[..n - mli]));
            }
            if !ok {
                r.zeroize();
                return Err(CipherError::KwFailed);
            }
            r.truncate(mli);
            Ok(r)
        }
    }
}

/// RFC 3217 §3.1's fixed IV of the second encryption pass.
const DES3_WRAP_IV2: [u8; 8] = [0x4a, 0xdd, 0xa2, 0x2c, 0x79, 0xe8, 0x21, 0x05];

fn cms_icv(cek: &[u8]) -> [u8; 8] {
    let h = purecrypto::hash::sha1(cek);
    h[..8].try_into().expect("8 bytes")
}

/// RFC 3217 §3.1: CEK || ICV, Triple-DES CBC under a random IV, prepended
/// with the IV, byte-reversed, then Triple-DES CBC under the fixed IV.
pub(super) fn des3_wrap<B: Bc>(c: &B, data: &[u8]) -> Result<Vec<u8>, CipherError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    if !data.len().is_multiple_of(8) {
        return Err(CipherError::OperationFailed);
    }
    let mut iv = [0u8; 8];
    crate::random_bytes(&mut iv);
    let mut t = Vec::with_capacity(data.len() + 16);
    t.extend_from_slice(&iv);
    t.extend_from_slice(data);
    t.extend_from_slice(&cms_icv(data));
    super::cbc_enc(c, &iv, &mut t[8..]);
    t.reverse();
    super::cbc_enc(c, &DES3_WRAP_IV2, &mut t);
    Ok(t)
}

/// RFC 3217 §3.2.
pub(super) fn des3_unwrap<B: Bc>(c: &B, data: &[u8]) -> Result<Vec<u8>, CipherError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    if !data.len().is_multiple_of(8) || data.len() < 24 {
        return Err(CipherError::OperationFailed);
    }
    let mut t = data.to_vec();
    super::cbc_dec(c, &DES3_WRAP_IV2, &mut t);
    t.reverse();
    let iv: [u8; 8] = t[..8].try_into().expect("8 bytes");
    let mut rest = t[8..].to_vec();
    t.zeroize();
    super::cbc_dec(c, &iv, &mut rest);
    let n = rest.len() - 8;
    let icv = cms_icv(&rest[..n]);
    // The verdict is the public outcome of unwrapping.
    if !bool::from(icv[..].ct_eq(&rest[n..])) {
        rest.zeroize();
        return Err(CipherError::OperationFailed);
    }
    rest.truncate(n);
    Ok(rest)
}
