//! OCB authenticated encryption, RFC 7253, over a 128-bit block cipher,
//! with nonces of 1 to 15 bytes and tags of 1 to 16 bytes (RFC 7253 §4.2
//! and §4.3; OpenSSL's `aes-*-ocb` accept the same ranges, observed on
//! PHP 8.5).

use purecrypto::ct::ConstantTimeEq;
use purecrypto::zeroize::Zeroize;

use super::Bc;

/// Blocks processed per batched cipher call.
const W: usize = 32;

pub(crate) struct Ocb<B: Bc> {
    c: B,
    /// L_*, L_$ and L_0, L_1, … (RFC 7253 §4.1), computed on demand.
    l_star: u128,
    l_dollar: u128,
    l: Vec<u128>,
}

#[inline]
fn double(s: u128) -> u128 {
    // RFC 7253 §2: doubling in GF(2^128) with x^128 + x^7 + x^2 + x + 1.
    (s << 1) ^ (0u128.wrapping_sub(s >> 127) & 0x87)
}

impl<B: Bc> Ocb<B> {
    pub(crate) fn new(c: B) -> Ocb<B> {
        let mut z = [0u8; 16];
        c.e(&mut z);
        let l_star = u128::from_be_bytes(z);
        let l_dollar = double(l_star);
        let l0 = double(l_dollar);
        Ocb {
            c,
            l_star,
            l_dollar,
            l: vec![l0],
        }
    }

    fn l_at(&mut self, i: usize) -> u128 {
        while self.l.len() <= i {
            let last = *self.l.last().expect("L_0");
            self.l.push(double(last));
        }
        self.l[i]
    }

    fn enc(&self, x: u128) -> u128 {
        let mut b = x.to_be_bytes();
        self.c.e(&mut b);
        u128::from_be_bytes(b)
    }

    /// Offset_0 from the nonce (RFC 7253 §4.2).
    fn offset0(&self, nonce: &[u8], tag_len: usize) -> u128 {
        let mut n = [0u8; 16];
        n[16 - nonce.len()..].copy_from_slice(nonce);
        n[15 - nonce.len()] |= 1;
        n[0] |= (((tag_len * 8) % 128) as u8) << 1;
        let bottom = (n[15] & 0x3f) as u32;
        n[15] &= 0xc0;
        let ktop = self.enc(u128::from_be_bytes(n));
        let stretch_lo = ((ktop >> 64) as u64) ^ ((ktop >> 56) as u64);
        if bottom == 0 {
            ktop
        } else {
            (ktop << bottom) | (stretch_lo >> (64 - bottom)) as u128
        }
    }

    /// HASH(K, A) (RFC 7253 §4.1).
    fn hash(&mut self, a: &[u8]) -> u128 {
        let mut sum = 0u128;
        let mut off = 0u128;
        let full = a.len() / 16;
        let mut buf = [0u8; 16 * W];
        let mut i = 0;
        while i < full {
            let nb = (full - i).min(W);
            for j in 0..nb {
                off ^= self.l_at((i + j + 1).trailing_zeros() as usize);
                let x = u128::from_be_bytes(a[(i + j) * 16..(i + j + 1) * 16].try_into().unwrap())
                    ^ off;
                buf[j * 16..j * 16 + 16].copy_from_slice(&x.to_be_bytes());
            }
            self.c.e_many(&mut buf[..nb * 16]);
            for j in 0..nb {
                sum ^= u128::from_be_bytes(buf[j * 16..j * 16 + 16].try_into().unwrap());
            }
            i += nb;
        }
        let rest = &a[full * 16..];
        if !rest.is_empty() {
            off ^= self.l_star;
            let mut p = [0u8; 16];
            p[..rest.len()].copy_from_slice(rest);
            p[rest.len()] = 0x80;
            sum ^= self.enc(u128::from_be_bytes(p) ^ off);
        }
        buf.zeroize();
        sum
    }

    /// Encrypts `buf` in place and returns the tag (RFC 7253 §4.2).
    pub(crate) fn encrypt(
        &mut self,
        nonce: &[u8],
        aad: &[u8],
        buf: &mut [u8],
        tag_len: usize,
    ) -> Vec<u8> {
        let (checksum, off) = self.crypt(nonce, buf, tag_len, false);
        let tag = self.enc(checksum ^ off ^ self.l_dollar) ^ self.hash(aad);
        tag.to_be_bytes()[..tag_len].to_vec()
    }

    /// Verifies `tag` and decrypts `buf` in place (RFC 7253 §4.3); on a
    /// mismatch the buffer is wiped and `false` returned.
    pub(crate) fn decrypt(&mut self, nonce: &[u8], aad: &[u8], buf: &mut [u8], tag: &[u8]) -> bool {
        let (checksum, off) = self.crypt(nonce, buf, tag.len(), true);
        let full = self.enc(checksum ^ off ^ self.l_dollar) ^ self.hash(aad);
        // The verdict is the public outcome of decryption.
        let ok = bool::from(full.to_be_bytes()[..tag.len()].ct_eq(tag));
        if !ok {
            buf.zeroize();
        }
        ok
    }

    /// The en/decryption pass: returns Checksum and the final Offset.
    fn crypt(&mut self, nonce: &[u8], buf: &mut [u8], tag_len: usize, dec: bool) -> (u128, u128) {
        let mut off = self.offset0(nonce, tag_len);
        let mut checksum = 0u128;
        let full = buf.len() / 16;
        let mut offs = [0u128; W];
        let mut i = 0;
        while i < full {
            let nb = (full - i).min(W);
            let win = &mut buf[i * 16..(i + nb) * 16];
            for j in 0..nb {
                off ^= self.l_at((i + j + 1).trailing_zeros() as usize);
                offs[j] = off;
                let blk = &mut win[j * 16..j * 16 + 16];
                let x = u128::from_be_bytes((&*blk).try_into().unwrap());
                if !dec {
                    checksum ^= x;
                }
                blk.copy_from_slice(&(x ^ off).to_be_bytes());
            }
            if dec {
                self.c.d_many(win);
            } else {
                self.c.e_many(win);
            }
            for (j, o) in offs.iter().enumerate().take(nb) {
                let blk = &mut win[j * 16..j * 16 + 16];
                let y = u128::from_be_bytes((&*blk).try_into().unwrap()) ^ o;
                if dec {
                    checksum ^= y;
                }
                blk.copy_from_slice(&y.to_be_bytes());
            }
            i += nb;
        }
        let rest = &mut buf[full * 16..];
        if !rest.is_empty() {
            off ^= self.l_star;
            let pad = self.enc(off).to_be_bytes();
            let mut p = [0u8; 16];
            for (k, b) in rest.iter_mut().enumerate() {
                let x = *b;
                *b ^= pad[k];
                p[k] = if dec { *b } else { x };
            }
            p[rest.len()] = 0x80;
            checksum ^= u128::from_be_bytes(p);
        }
        (checksum, off)
    }
}
