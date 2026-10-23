//! EncipherImage() / DecipherImage(): the pixels are scrambled with AES in
//! counter mode, as ImageMagick documents: one half of the passphrase
//! keys the cipher, the other half hashed with the image dimensions makes
//! the counter's initial value; only the samples change (every channel,
//! alpha included, as 16-bit values), each row starting on a fresh
//! keystream block. Enciphering twice with the same passphrase restores
//! the image, so the two operations are the same.
//!
//! The key schedule is goro's own (SHA-256 of each half): the layout
//! follows ImageMagick's documentation, but images enciphered by
//! ImageMagick itself do not decipher here, nor the reverse.

use crate::error::{MResult, MagickError, severity};
use crate::image::Image;
use purecrypto::cipher::{Aes256, Ctr};
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
    let (k, rest) = passphrase.split_at(passphrase.len().div_ceil(2));
    let key = sha256(k);
    let mut seed = rest.to_vec();
    seed.extend_from_slice(&(img.columns as u64).to_le_bytes());
    seed.extend_from_slice(&(img.rows as u64).to_le_bytes());
    let digest = sha256(&seed);
    let mut iv = [0u8; 16];
    iv.copy_from_slice(&digest[..16]);
    let mut ctr = Ctr::new(Aes256::new(&key), &iv);
    let cmyk = img.black.is_some();
    let nch = 3 + cmyk as usize + img.alpha as usize;
    let mut row = vec![0u8; img.columns * nch * 2];
    for y in 0..img.rows {
        let mut k = 0;
        for x in 0..img.columns {
            let p = img.px(x, y);
            let mut put = |v: u16| {
                row[k..k + 2].copy_from_slice(&v.to_be_bytes());
                k += 2;
            };
            put(p[0]);
            put(p[1]);
            put(p[2]);
            if cmyk {
                put(img.black.as_ref().unwrap()[y * img.columns + x]);
            }
            if img.alpha {
                put(p[3]);
            }
        }
        // a whole number of keystream blocks per row
        let pad = (16 - row.len() % 16) % 16;
        ctr.apply_keystream(&mut row);
        ctr.apply_keystream(&mut vec![0u8; pad]);
        let mut k = 0;
        let mut get = || {
            let v = u16::from_be_bytes([row[k], row[k + 1]]);
            k += 2;
            v
        };
        for x in 0..img.columns {
            let mut p = img.px(x, y);
            p[0] = get();
            p[1] = get();
            p[2] = get();
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
    img.colors = None;
    Ok(())
}

pub fn decipher(img: &mut Image, passphrase: &[u8]) -> MResult<()> {
    encipher(img, passphrase)
}
