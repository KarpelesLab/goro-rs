//! EncipherImage() / DecipherImage(): the pixels are scrambled with AES in
//! counter mode, as ImageMagick documents, byte for byte as ImageMagick
//! 7.1.2 does it (established by black-box runs):
//! - the passphrase splits at half its length (rounded down); the second
//!   half is the AES key: AES-256 from its first 32 bytes when it has
//!   that many, else AES-192 from 24, else AES-128 zero-padded to 16;
//! - the counter starts at the first 16 bytes of SHA-256 over the first
//!   half followed by the pixel count (columns × rows) as a 64-bit
//!   little-endian integer, and counts up as a 128-bit big-endian number;
//! - the keystream is XORed into each row's samples, every channel of the
//!   image (gray images have one, CMYK four, alpha last) exported at the
//!   image depth, packed most significant bit first (8-bit images one
//!   byte a sample, 16-bit ones big-endian values); each row starts on a
//!   fresh keystream block.
//!
//! Enciphering twice with the same passphrase restores the image, so the
//! two operations are the same.

use crate::error::{MResult, MagickError, severity};
use crate::image::Image;
use crate::types::Colorspace;
use purecrypto::cipher::{Aes128, Aes192, Aes256, BlockCipher, Ctr};
use purecrypto::hash::sha256;

pub fn encipher(img: &mut Image, passphrase: &[u8]) -> MResult<()> {
    if passphrase.is_empty() {
        return Err(MagickError::with_desc(
            severity::OPTION_ERROR,
            "missing passphrase",
            "",
            "error/cipher.c/PasskeyEncipherImage",
        ));
    }
    let (nonce_part, key_part) = passphrase.split_at(passphrase.len() / 2);
    let mut seed = nonce_part.to_vec();
    seed.extend_from_slice(&((img.columns * img.rows) as u64).to_le_bytes());
    let digest = sha256(&seed);
    let mut iv = [0u8; 16];
    iv.copy_from_slice(&digest[..16]);
    if key_part.len() >= 32 {
        let k: [u8; 32] = key_part[..32].try_into().unwrap();
        scramble(img, Ctr::new(Aes256::new(&k), &iv));
    } else if key_part.len() >= 24 {
        let k: [u8; 24] = key_part[..24].try_into().unwrap();
        scramble(img, Ctr::new(Aes192::new(&k), &iv));
    } else {
        let mut k = [0u8; 16];
        let n = key_part.len().min(16);
        k[..n].copy_from_slice(&key_part[..n]);
        scramble(img, Ctr::new(Aes128::new(&k), &iv));
    }
    img.colors = None;
    Ok(())
}

/// XOR the keystream into the image's exported rows.
fn scramble<C: BlockCipher>(img: &mut Image, mut ctr: Ctr<C>) {
    let gray = img.colorspace == Colorspace::Gray;
    let cmyk = img.black.is_some();
    let colour = if gray { 1 } else { 3 };
    let nch = colour + cmyk as usize + img.alpha as usize;
    // the samples are packed at the image depth, most significant bit first
    let depth = img.depth.clamp(1, 16) as usize;
    let max = ((1u32 << depth) - 1) as f64;
    let mut samples = vec![0u16; img.columns * nch];
    let mut row = vec![0u8; (samples.len() * depth).div_ceil(8)];
    let pad = (16 - row.len() % 16) % 16;
    let mut skip = vec![0u8; pad];
    for y in 0..img.rows {
        let mut k = 0;
        for x in 0..img.columns {
            let p = img.px(x, y);
            let mut put = |v: u16| {
                samples[k] = (v as f64 * max / 65535.0).round() as u16;
                k += 1;
            };
            for c in 0..colour {
                put(p[c]);
            }
            if cmyk {
                put(img.black.as_ref().unwrap()[y * img.columns + x]);
            }
            if img.alpha {
                put(p[3]);
            }
        }
        row.fill(0);
        for (i, &v) in samples.iter().enumerate() {
            for b in 0..depth {
                if v >> (depth - 1 - b) & 1 != 0 {
                    let bit = i * depth + b;
                    row[bit / 8] |= 0x80 >> (bit % 8);
                }
            }
        }
        ctr.apply_keystream(&mut row);
        // the rest of the row's last keystream block goes unused
        ctr.apply_keystream(&mut skip);
        for (i, v) in samples.iter_mut().enumerate() {
            let mut s = 0u16;
            for b in 0..depth {
                let bit = i * depth + b;
                s = s << 1 | ((row[bit / 8] >> (7 - bit % 8)) & 1) as u16;
            }
            *v = (s as f64 * 65535.0 / max).round() as u16;
        }
        let mut k = 0;
        let mut get = || {
            k += 1;
            samples[k - 1]
        };
        for x in 0..img.columns {
            let mut p = img.px(x, y);
            for c in 0..colour {
                p[c] = get();
            }
            if gray {
                p[1] = p[0];
                p[2] = p[0];
            }
            if cmyk {
                let v = get();
                img.black.as_mut().unwrap()[y * img.columns + x] = v;
            }
            if img.alpha {
                p[3] = get();
            }
            img.set_px(x, y, p);
        }
    }
}

pub fn decipher(img: &mut Image, passphrase: &[u8]) -> MResult<()> {
    encipher(img, passphrase)
}
