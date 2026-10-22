//! The Salsa20 / ChaCha20 cores purecrypto does not expose: Salsa20 (with
//! its 64-bit block counter), HSalsa20, XSalsa20 and HChaCha20
//! (libsodium's crypto_core_salsa20, crypto_core_hsalsa20,
//! crypto_stream_salsa20_xor_ic, crypto_stream_xsalsa20_xor_ic and
//! crypto_core_hchacha20), plus the original (djb, 64-bit nonce and
//! counter) and IETF ChaCha20 stream ciphers built on purecrypto's block
//! function.

use purecrypto::cipher::ChaCha20;

/// "expand 32-byte k"
const SIGMA: [u32; 4] = [0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574];

fn load32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn salsa_rounds(x: &mut [u32; 16]) {
    for _ in 0..10 {
        // column round
        x[4] ^= x[0].wrapping_add(x[12]).rotate_left(7);
        x[8] ^= x[4].wrapping_add(x[0]).rotate_left(9);
        x[12] ^= x[8].wrapping_add(x[4]).rotate_left(13);
        x[0] ^= x[12].wrapping_add(x[8]).rotate_left(18);
        x[9] ^= x[5].wrapping_add(x[1]).rotate_left(7);
        x[13] ^= x[9].wrapping_add(x[5]).rotate_left(9);
        x[1] ^= x[13].wrapping_add(x[9]).rotate_left(13);
        x[5] ^= x[1].wrapping_add(x[13]).rotate_left(18);
        x[14] ^= x[10].wrapping_add(x[6]).rotate_left(7);
        x[2] ^= x[14].wrapping_add(x[10]).rotate_left(9);
        x[6] ^= x[2].wrapping_add(x[14]).rotate_left(13);
        x[10] ^= x[6].wrapping_add(x[2]).rotate_left(18);
        x[3] ^= x[15].wrapping_add(x[11]).rotate_left(7);
        x[7] ^= x[3].wrapping_add(x[15]).rotate_left(9);
        x[11] ^= x[7].wrapping_add(x[3]).rotate_left(13);
        x[15] ^= x[11].wrapping_add(x[7]).rotate_left(18);
        // row round
        x[1] ^= x[0].wrapping_add(x[3]).rotate_left(7);
        x[2] ^= x[1].wrapping_add(x[0]).rotate_left(9);
        x[3] ^= x[2].wrapping_add(x[1]).rotate_left(13);
        x[0] ^= x[3].wrapping_add(x[2]).rotate_left(18);
        x[6] ^= x[5].wrapping_add(x[4]).rotate_left(7);
        x[7] ^= x[6].wrapping_add(x[5]).rotate_left(9);
        x[4] ^= x[7].wrapping_add(x[6]).rotate_left(13);
        x[5] ^= x[4].wrapping_add(x[7]).rotate_left(18);
        x[11] ^= x[10].wrapping_add(x[9]).rotate_left(7);
        x[8] ^= x[11].wrapping_add(x[10]).rotate_left(9);
        x[9] ^= x[8].wrapping_add(x[11]).rotate_left(13);
        x[10] ^= x[9].wrapping_add(x[8]).rotate_left(18);
        x[12] ^= x[15].wrapping_add(x[14]).rotate_left(7);
        x[13] ^= x[12].wrapping_add(x[15]).rotate_left(9);
        x[14] ^= x[13].wrapping_add(x[12]).rotate_left(13);
        x[15] ^= x[14].wrapping_add(x[13]).rotate_left(18);
    }
}

/// The Salsa20 input block: constants, key and the 16-byte "input"
/// (nonce + counter, or the HSalsa20 nonce).
fn salsa_state(key: &[u8; 32], input: &[u8; 16]) -> [u32; 16] {
    [
        SIGMA[0],
        load32(&key[0..]),
        load32(&key[4..]),
        load32(&key[8..]),
        load32(&key[12..]),
        SIGMA[1],
        load32(&input[0..]),
        load32(&input[4..]),
        load32(&input[8..]),
        load32(&input[12..]),
        SIGMA[2],
        load32(&key[16..]),
        load32(&key[20..]),
        load32(&key[24..]),
        load32(&key[28..]),
        SIGMA[3],
    ]
}

/// crypto_core_hsalsa20() with the default constants.
pub fn hsalsa20(key: &[u8; 32], nonce16: &[u8; 16]) -> [u8; 32] {
    let mut x = salsa_state(key, nonce16);
    salsa_rounds(&mut x);
    let mut out = [0u8; 32];
    for (i, &w) in [x[0], x[5], x[10], x[15], x[6], x[7], x[8], x[9]]
        .iter()
        .enumerate()
    {
        out[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    out
}

fn salsa20_block(key: &[u8; 32], nonce8: &[u8; 8], counter: u64) -> [u8; 64] {
    let mut input = [0u8; 16];
    input[..8].copy_from_slice(nonce8);
    input[8..].copy_from_slice(&counter.to_le_bytes());
    let j = salsa_state(key, &input);
    let mut x = j;
    salsa_rounds(&mut x);
    let mut out = [0u8; 64];
    for i in 0..16 {
        out[i * 4..i * 4 + 4].copy_from_slice(&x[i].wrapping_add(j[i]).to_le_bytes());
    }
    out
}

/// crypto_stream_salsa20_xor_ic(): `buf ^= Salsa20(key, nonce)` from block
/// `ic` on (the counter wraps around 2^64 like libsodium's).
pub fn salsa20_xor_ic(buf: &mut [u8], nonce8: &[u8; 8], ic: u64, key: &[u8; 32]) {
    let mut ctr = ic;
    for chunk in buf.chunks_mut(64) {
        let ks = salsa20_block(key, nonce8, ctr);
        for (b, k) in chunk.iter_mut().zip(ks.iter()) {
            *b ^= k;
        }
        ctr = ctr.wrapping_add(1);
    }
}

/// crypto_stream_xsalsa20_xor_ic()
pub fn xsalsa20_xor_ic(buf: &mut [u8], nonce: &[u8; 24], ic: u64, key: &[u8; 32]) {
    let subkey = hsalsa20(key, nonce[..16].try_into().unwrap());
    salsa20_xor_ic(buf, nonce[16..].try_into().unwrap(), ic, &subkey);
}

fn chacha_quarter(x: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    x[a] = x[a].wrapping_add(x[b]);
    x[d] = (x[d] ^ x[a]).rotate_left(16);
    x[c] = x[c].wrapping_add(x[d]);
    x[b] = (x[b] ^ x[c]).rotate_left(12);
    x[a] = x[a].wrapping_add(x[b]);
    x[d] = (x[d] ^ x[a]).rotate_left(8);
    x[c] = x[c].wrapping_add(x[d]);
    x[b] = (x[b] ^ x[c]).rotate_left(7);
}

/// crypto_core_hchacha20() with the default constants.
pub fn hchacha20(key: &[u8; 32], nonce16: &[u8; 16]) -> [u8; 32] {
    let mut x = [0u32; 16];
    x[..4].copy_from_slice(&SIGMA);
    for i in 0..8 {
        x[4 + i] = load32(&key[i * 4..]);
    }
    for i in 0..4 {
        x[12 + i] = load32(&nonce16[i * 4..]);
    }
    for _ in 0..10 {
        chacha_quarter(&mut x, 0, 4, 8, 12);
        chacha_quarter(&mut x, 1, 5, 9, 13);
        chacha_quarter(&mut x, 2, 6, 10, 14);
        chacha_quarter(&mut x, 3, 7, 11, 15);
        chacha_quarter(&mut x, 0, 5, 10, 15);
        chacha_quarter(&mut x, 1, 6, 11, 12);
        chacha_quarter(&mut x, 2, 7, 8, 13);
        chacha_quarter(&mut x, 3, 4, 9, 14);
    }
    let mut out = [0u8; 32];
    for (i, &w) in x[..4].iter().chain(x[12..].iter()).enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    out
}

/// crypto_stream_chacha20_xor_ic(): the original ChaCha20 (64-bit nonce,
/// 64-bit block counter). Its state words 12..15 are (counter low, counter
/// high, nonce), i.e. the IETF layout with a 12-byte "nonce" made of the
/// counter's high word followed by the 8-byte nonce.
pub fn chacha20_xor_ic(buf: &mut [u8], nonce8: &[u8; 8], ic: u64, key: &[u8; 32]) {
    let c = ChaCha20::new(key);
    let mut ctr = ic;
    let mut n12 = [0u8; 12];
    n12[4..].copy_from_slice(nonce8);
    for chunk in buf.chunks_mut(64) {
        n12[..4].copy_from_slice(&((ctr >> 32) as u32).to_le_bytes());
        let ks = c.block(&n12, ctr as u32);
        for (b, k) in chunk.iter_mut().zip(ks.iter()) {
            *b ^= k;
        }
        ctr = ctr.wrapping_add(1);
    }
}

/// crypto_stream_chacha20_ietf_xor_ic(): the IETF ChaCha20 (96-bit nonce,
/// 32-bit block counter).
pub fn chacha20_ietf_xor_ic(buf: &mut [u8], nonce: &[u8; 12], ic: u32, key: &[u8; 32]) {
    ChaCha20::new(key).apply_keystream(nonce, ic, buf);
}

/// crypto_stream_xchacha20_xor_ic()
pub fn xchacha20_xor_ic(buf: &mut [u8], nonce: &[u8; 24], ic: u64, key: &[u8; 32]) {
    let subkey = hchacha20(key, nonce[..16].try_into().unwrap());
    chacha20_xor_ic(buf, nonce[16..].try_into().unwrap(), ic, &subkey);
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
    fn hsalsa20_vector() {
        // NaCl's core1 test: the crypto_box shared secret's HSalsa20.
        let shared = hex("4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742");
        let k = hsalsa20(shared[..].try_into().unwrap(), &[0u8; 16]);
        assert_eq!(
            k.to_vec(),
            hex("1b27556473e985d462cd51197a9a46c76009549eac6474f206c4ee0844f68389")
        );
    }

    #[test]
    fn hchacha20_vector() {
        // draft-irtf-cfrg-xchacha 2.2.1
        let key = hex("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f");
        let nonce = hex("000000090000004a0000000031415927");
        let k = hchacha20(key[..].try_into().unwrap(), nonce[..].try_into().unwrap());
        assert_eq!(
            k.to_vec(),
            hex("82413b4227b27bfed30e42508a877d73a0f9e4d58a74a853c12ec41326d3ecdc")
        );
    }

    #[test]
    fn xsalsa20_vector() {
        // NaCl stream3 test: the first 32 bytes of crypto_stream().
        let key = hex("1b27556473e985d462cd51197a9a46c76009549eac6474f206c4ee0844f68389");
        let nonce = hex("69696ee955b62b73cd62bda875fc73d68219e0036b7a0b37");
        let mut buf = [0u8; 32];
        xsalsa20_xor_ic(
            &mut buf,
            nonce[..].try_into().unwrap(),
            0,
            key[..].try_into().unwrap(),
        );
        assert_eq!(
            buf.to_vec(),
            hex("eea6a7251c1e72916d11c2cb214d3c252539121d8e234e652d651fa4c8cff880")
        );
    }
}
