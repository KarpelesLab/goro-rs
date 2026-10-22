//! BLAKE2b as libsodium implements it (crypto_generichash_blake2b,
//! blake2b-ref.c): keyed, salted and personalized hashing, and a state
//! whose byte layout is libsodium's crypto_generichash_state (PHP hands it
//! to scripts as a string: h[8], t[2], f[2], buf[256], buflen, last_node,
//! padded to 384 bytes).
//!
//! purecrypto's Blake2bMac covers keyed hashing only (no salt/personal
//! parameters, no serializable state).

pub const OUTBYTES: usize = 64;
pub const KEYBYTES: usize = 64;
pub const BLOCKBYTES: usize = 128;
/// sizeof(crypto_generichash_state)
pub const STATE_BYTES: usize = 384;

const IV: [u64; 8] = [
    0x6a09e667f3bcc908,
    0xbb67ae8584caa73b,
    0x3c6ef372fe94f82b,
    0xa54ff53a5f1d36f1,
    0x510e527fade682d1,
    0x9b05688c2b3e6c1f,
    0x1f83d9abfb41bd6b,
    0x5be0cd19137e2179,
];

const SIGMA: [[usize; 16]; 12] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
];

#[derive(Clone)]
pub struct Blake2b {
    h: [u64; 8],
    t: [u64; 2],
    f: [u64; 2],
    buf: [u8; 2 * BLOCKBYTES],
    buflen: usize,
    last_node: u8,
}

impl Drop for Blake2b {
    fn drop(&mut self) {
        self.h = [0; 8];
        self.buf = [0; 2 * BLOCKBYTES];
    }
}

impl Blake2b {
    /// blake2b_init_param() + the key block of blake2b_init_key(): `outlen`
    /// in 1..=64, `key` at most 64 bytes, `salt`/`personal` 16 bytes or
    /// absent (zeros). Err(()) for out-of-range lengths.
    pub fn new(
        outlen: usize,
        key: &[u8],
        salt: Option<&[u8; 16]>,
        personal: Option<&[u8; 16]>,
    ) -> Result<Blake2b, ()> {
        if outlen == 0 || outlen > OUTBYTES || key.len() > KEYBYTES {
            return Err(());
        }
        let mut p = [0u8; 64];
        p[0] = outlen as u8;
        p[1] = key.len() as u8;
        p[2] = 1; // fanout
        p[3] = 1; // depth
        if let Some(s) = salt {
            p[32..48].copy_from_slice(s);
        }
        if let Some(s) = personal {
            p[48..64].copy_from_slice(s);
        }
        let mut h = IV;
        for (i, w) in h.iter_mut().enumerate() {
            *w ^= u64::from_le_bytes(p[i * 8..i * 8 + 8].try_into().unwrap());
        }
        let mut s = Blake2b {
            h,
            t: [0; 2],
            f: [0; 2],
            buf: [0; 2 * BLOCKBYTES],
            buflen: 0,
            last_node: 0,
        };
        if !key.is_empty() {
            let mut block = [0u8; BLOCKBYTES];
            block[..key.len()].copy_from_slice(key);
            s.update(&block);
            block.fill(0);
        }
        Ok(s)
    }

    fn increment_counter(&mut self, inc: u64) {
        self.t[0] = self.t[0].wrapping_add(inc);
        if self.t[0] < inc {
            self.t[1] = self.t[1].wrapping_add(1);
        }
    }

    fn compress(&mut self, block: &[u8]) {
        let mut m = [0u64; 16];
        for (i, w) in m.iter_mut().enumerate() {
            *w = u64::from_le_bytes(block[i * 8..i * 8 + 8].try_into().unwrap());
        }
        let mut v = [0u64; 16];
        v[..8].copy_from_slice(&self.h);
        v[8..].copy_from_slice(&IV);
        v[12] ^= self.t[0];
        v[13] ^= self.t[1];
        v[14] ^= self.f[0];
        v[15] ^= self.f[1];
        fn g(v: &mut [u64; 16], a: usize, b: usize, c: usize, d: usize, x: u64, y: u64) {
            v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
            v[d] = (v[d] ^ v[a]).rotate_right(32);
            v[c] = v[c].wrapping_add(v[d]);
            v[b] = (v[b] ^ v[c]).rotate_right(24);
            v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
            v[d] = (v[d] ^ v[a]).rotate_right(16);
            v[c] = v[c].wrapping_add(v[d]);
            v[b] = (v[b] ^ v[c]).rotate_right(63);
        }
        for s in &SIGMA {
            g(&mut v, 0, 4, 8, 12, m[s[0]], m[s[1]]);
            g(&mut v, 1, 5, 9, 13, m[s[2]], m[s[3]]);
            g(&mut v, 2, 6, 10, 14, m[s[4]], m[s[5]]);
            g(&mut v, 3, 7, 11, 15, m[s[6]], m[s[7]]);
            g(&mut v, 0, 5, 10, 15, m[s[8]], m[s[9]]);
            g(&mut v, 1, 6, 11, 12, m[s[10]], m[s[11]]);
            g(&mut v, 2, 7, 8, 13, m[s[12]], m[s[13]]);
            g(&mut v, 3, 4, 9, 14, m[s[14]], m[s[15]]);
        }
        for i in 0..8 {
            self.h[i] ^= v[i] ^ v[i + 8];
        }
    }

    /// blake2b_update(): lazy, keeps up to two blocks buffered.
    pub fn update(&mut self, mut data: &[u8]) {
        while !data.is_empty() {
            let left = self.buflen;
            let fill = 2 * BLOCKBYTES - left;
            if data.len() > fill {
                self.buf[left..].copy_from_slice(&data[..fill]);
                self.buflen += fill;
                self.increment_counter(BLOCKBYTES as u64);
                let block: [u8; BLOCKBYTES] = self.buf[..BLOCKBYTES].try_into().unwrap();
                self.compress(&block);
                self.buf.copy_within(BLOCKBYTES.., 0);
                self.buflen -= BLOCKBYTES;
                data = &data[fill..];
            } else {
                self.buf[left..left + data.len()].copy_from_slice(data);
                self.buflen += data.len();
                data = &[];
            }
        }
    }

    /// blake2b_final(): the first `out.len()` (1..=64) bytes of the digest.
    /// Err(()) once the state was finalized already.
    pub fn finalize(&mut self, out: &mut [u8]) -> Result<(), ()> {
        if out.is_empty() || out.len() > OUTBYTES || self.f[0] != 0 {
            return Err(());
        }
        if self.buflen > BLOCKBYTES {
            self.increment_counter(BLOCKBYTES as u64);
            let block: [u8; BLOCKBYTES] = self.buf[..BLOCKBYTES].try_into().unwrap();
            self.compress(&block);
            self.buflen -= BLOCKBYTES;
            self.buf
                .copy_within(BLOCKBYTES..BLOCKBYTES + self.buflen, 0);
        }
        self.increment_counter(self.buflen as u64);
        // blake2b_set_lastblock()
        if self.last_node != 0 {
            self.f[1] = u64::MAX;
        }
        self.f[0] = u64::MAX;
        let n = self.buflen;
        self.buf[n..].fill(0);
        let block: [u8; BLOCKBYTES] = self.buf[..BLOCKBYTES].try_into().unwrap();
        self.compress(&block);
        let mut full = [0u8; OUTBYTES];
        for (i, w) in self.h.iter().enumerate() {
            full[i * 8..i * 8 + 8].copy_from_slice(&w.to_le_bytes());
        }
        out.copy_from_slice(&full[..out.len()]);
        full.fill(0);
        Ok(())
    }

    /// The state as libsodium's crypto_generichash_state bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = vec![0u8; STATE_BYTES];
        for (i, w) in self.h.iter().enumerate() {
            out[i * 8..i * 8 + 8].copy_from_slice(&w.to_le_bytes());
        }
        out[64..72].copy_from_slice(&self.t[0].to_le_bytes());
        out[72..80].copy_from_slice(&self.t[1].to_le_bytes());
        out[80..88].copy_from_slice(&self.f[0].to_le_bytes());
        out[88..96].copy_from_slice(&self.f[1].to_le_bytes());
        out[96..352].copy_from_slice(&self.buf);
        out[352..360].copy_from_slice(&(self.buflen as u64).to_le_bytes());
        out[360] = self.last_node;
        out
    }

    /// A state from crypto_generichash_state bytes (STATE_BYTES long).
    /// Err(()) for a buffered length libsodium would overrun its buffer
    /// with.
    pub fn from_bytes(b: &[u8]) -> Result<Blake2b, ()> {
        if b.len() != STATE_BYTES {
            return Err(());
        }
        let w = |o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
        let buflen = w(352);
        if buflen > (2 * BLOCKBYTES) as u64 {
            return Err(());
        }
        let mut h = [0u64; 8];
        for (i, x) in h.iter_mut().enumerate() {
            *x = w(i * 8);
        }
        Ok(Blake2b {
            h,
            t: [w(64), w(72)],
            f: [w(80), w(88)],
            buf: b[96..352].try_into().unwrap(),
            buflen: buflen as usize,
            last_node: b[360],
        })
    }
}

/// crypto_generichash_blake2b_salt_personal() (and crypto_generichash()
/// without salt/personal).
pub fn hash(
    out: &mut [u8],
    data: &[u8],
    key: &[u8],
    salt: Option<&[u8; 16]>,
    personal: Option<&[u8; 16]>,
) -> Result<(), ()> {
    let mut s = Blake2b::new(out.len(), key, salt, personal)?;
    s.update(data);
    s.finalize(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{:02x}", x)).collect()
    }

    #[test]
    fn rfc7693_abc() {
        let mut out = [0u8; 64];
        hash(&mut out, b"abc", b"", None, None).unwrap();
        assert_eq!(
            hex(&out),
            "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d1\
             7d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923"
        );
    }

    #[test]
    fn streaming_matches_oneshot() {
        let data: Vec<u8> = (0..1000u32).map(|i| i as u8).collect();
        let key = [7u8; 32];
        let mut one = [0u8; 32];
        hash(&mut one, &data, &key, None, None).unwrap();
        for split in [0usize, 1, 127, 128, 129, 256, 257, 999] {
            let mut s = Blake2b::new(32, &key, None, None).unwrap();
            s.update(&data[..split]);
            let mut s = Blake2b::from_bytes(&s.to_bytes()).unwrap();
            s.update(&data[split..]);
            let mut out = [0u8; 32];
            s.finalize(&mut out).unwrap();
            assert_eq!(out, one);
        }
    }
}
