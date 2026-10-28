//! ChaCha20 helpers on purecrypto's RFC 8439 ChaCha20: HChaCha20
//! (draft-irtf-cfrg-xchacha §2.2), the original 64-bit-nonce/64-bit-counter
//! ChaCha20 (Bernstein, "ChaCha, a variant of Salsa20") and XChaCha20.
//!
//! The original layout puts a 64-bit block counter in state words 12–13
//! and the 8-byte nonce in words 14–15; RFC 8439 has a 32-bit counter in
//! word 12 and a 12-byte nonce in words 13–15. So the original cipher at
//! counter `c` is RFC 8439's with nonce `le32(c >> 32) ‖ nonce` and counter
//! `c mod 2^32`, as long as the low word does not wrap inside one call:
//! [`xor_ic`] splits at those boundaries.

use purecrypto::cipher::ChaCha20;

const SIGMA: [u32; 4] = [0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574];

macro_rules! qr {
    ($x:ident, $a:expr, $b:expr, $c:expr, $d:expr) => {
        $x[$a] = $x[$a].wrapping_add($x[$b]);
        $x[$d] = ($x[$d] ^ $x[$a]).rotate_left(16);
        $x[$c] = $x[$c].wrapping_add($x[$d]);
        $x[$b] = ($x[$b] ^ $x[$c]).rotate_left(12);
        $x[$a] = $x[$a].wrapping_add($x[$b]);
        $x[$d] = ($x[$d] ^ $x[$a]).rotate_left(8);
        $x[$c] = $x[$c].wrapping_add($x[$d]);
        $x[$b] = ($x[$b] ^ $x[$c]).rotate_left(7);
    };
}

/// HChaCha20: a 32-byte subkey from a key and a 16-byte input.
pub fn hchacha20(key: &[u8; 32], n: &[u8; 16]) -> [u8; 32] {
    let mut x = [0u32; 16];
    x[..4].copy_from_slice(&SIGMA);
    for i in 0..8 {
        x[4 + i] = u32::from_le_bytes(key[4 * i..4 * i + 4].try_into().unwrap());
    }
    for i in 0..4 {
        x[12 + i] = u32::from_le_bytes(n[4 * i..4 * i + 4].try_into().unwrap());
    }
    for _ in 0..10 {
        qr!(x, 0, 4, 8, 12);
        qr!(x, 1, 5, 9, 13);
        qr!(x, 2, 6, 10, 14);
        qr!(x, 3, 7, 11, 15);
        qr!(x, 0, 5, 10, 15);
        qr!(x, 1, 6, 11, 12);
        qr!(x, 2, 7, 8, 13);
        qr!(x, 3, 4, 9, 14);
    }
    let mut out = [0u8; 32];
    for (i, w) in x[..4].iter().chain(&x[12..]).enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
    }
    out
}

/// XORs the original-ChaCha20 keystream (8-byte nonce, 64-bit block
/// counter starting at `ic`) into `data`. The counter wraps modulo 2^64.
pub fn xor_ic(c: &ChaCha20, nonce: &[u8; 8], ic: u64, mut data: &mut [u8]) {
    let mut ctr = ic;
    while !data.is_empty() {
        let lo = ctr as u32;
        // Blocks left before the low word wraps.
        let room = (1u64 << 32) - lo as u64;
        let max = room.saturating_mul(64).min(usize::MAX as u64) as usize;
        let n = data.len().min(max);
        let mut n12 = [0u8; 12];
        n12[..4].copy_from_slice(&((ctr >> 32) as u32).to_le_bytes());
        n12[4..].copy_from_slice(nonce);
        let (now, rest) = data.split_at_mut(n);
        c.apply_keystream(&n12, lo, now);
        ctr = ctr.wrapping_add(n.div_ceil(64) as u64);
        data = rest;
    }
}

/// The first 64-byte block of the original ChaCha20 at counter 0 (its first
/// 32 bytes are the Poly1305 key of the original AEAD construction).
pub fn block0(c: &ChaCha20, nonce: &[u8; 8]) -> [u8; 64] {
    let mut n12 = [0u8; 12];
    n12[4..].copy_from_slice(nonce);
    c.block(&n12, 0)
}

/// XChaCha20 subkey and 8-byte nonce for a 24-byte nonce.
pub fn xchacha20_key(key: &[u8; 32], nonce: &[u8; 24]) -> ([u8; 32], [u8; 8]) {
    let sub = hchacha20(key, nonce[..16].try_into().unwrap());
    (sub, nonce[16..].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hchacha20_draft_vector() {
        // draft-irtf-cfrg-xchacha-03 §2.2.1.
        let key: [u8; 32] = core::array::from_fn(|i| i as u8);
        let n: [u8; 16] = [
            0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x4a, 0x00, 0x00, 0x00, 0x00, 0x31, 0x41,
            0x59, 0x27,
        ];
        let out = hchacha20(&key, &n);
        let want = [
            0x82, 0x41, 0x3b, 0x42, 0x27, 0xb2, 0x7b, 0xfe, 0xd3, 0x0e, 0x42, 0x50, 0x8a, 0x87,
            0x7d, 0x73, 0xa0, 0xf9, 0xe4, 0xd5, 0x8a, 0x74, 0xa8, 0x53, 0xc1, 0x2e, 0xc4, 0x13,
            0x26, 0xd3, 0xec, 0xdc,
        ];
        assert_eq!(out, want);
    }

    #[test]
    fn counter_wrap_split() {
        let c = ChaCha20::new(&[3; 32]);
        let nonce = [1u8; 8];
        let mut a = vec![0u8; 64 * 3];
        xor_ic(&c, &nonce, 0xffff_ffff, &mut a);
        let mut b = vec![0u8; 64 * 3];
        for (i, blk) in b.chunks_mut(64).enumerate() {
            let ctr = 0xffff_ffffu64 + i as u64;
            let mut part = vec![0u8; 64];
            xor_ic(&c, &nonce, ctr, &mut part);
            blk.copy_from_slice(&part);
        }
        assert_eq!(a, b);
        // Word 13 carries: block 2^32 uses nonce word 1.
        let mut n12 = [0u8; 12];
        n12[..4].copy_from_slice(&1u32.to_le_bytes());
        n12[4..].copy_from_slice(&nonce);
        assert_eq!(&a[64..128], &c.block(&n12, 0)[..]);
    }
}
