//! BLAKE2b (RFC 7693) with the full parameter block (key, salt,
//! personalization) and a streaming state that serializes to the
//! 384-byte string `sodium_crypto_generichash_init()` returns.
//!
//! The state layout and its buffering were observed on PHP 8.5 (libsodium
//! 1.0.18): the chaining value `h[0..8]`, the counter `t[0..2]` and the
//! finalization flags `f[0..2]` as little-endian 64-bit words, a 256-byte
//! buffer, the buffered length as a 64-bit word, a last-node byte and zero
//! padding to 384 bytes. A block is compressed only once more than 256
//! bytes are pending, so up to 256 bytes stay buffered; a keyed state
//! starts with the zero-padded key block buffered. Matching the layout
//! byte for byte keeps states interchangeable with PHP's.
//!
//! The arithmetic is data-independent (additions, rotations and XORs on
//! fixed message schedules), so it is constant-time.

/// RFC 7693 §2.6 initialization vector.
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

/// RFC 7693 §2.7 message schedule.
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

pub(crate) const BLOCK: usize = 128;
/// Size of the serialized state.
pub const STATE_BYTES: usize = 384;

macro_rules! g {
    ($v:ident, $a:expr, $b:expr, $c:expr, $d:expr, $x:expr, $y:expr) => {
        $v[$a] = $v[$a].wrapping_add($v[$b]).wrapping_add($x);
        $v[$d] = ($v[$d] ^ $v[$a]).rotate_right(32);
        $v[$c] = $v[$c].wrapping_add($v[$d]);
        $v[$b] = ($v[$b] ^ $v[$c]).rotate_right(24);
        $v[$a] = $v[$a].wrapping_add($v[$b]).wrapping_add($y);
        $v[$d] = ($v[$d] ^ $v[$a]).rotate_right(16);
        $v[$c] = $v[$c].wrapping_add($v[$d]);
        $v[$b] = ($v[$b] ^ $v[$c]).rotate_right(63);
    };
}

/// RFC 7693 §3.2 compression function `F`.
#[inline(always)]
fn compress(h: &mut [u64; 8], block: &[u8], t0: u64, t1: u64, f0: u64, f1: u64) {
    let mut m = [0u64; 16];
    for (w, c) in m.iter_mut().zip(block.as_chunks::<8>().0) {
        *w = u64::from_le_bytes(*c);
    }
    let mut v = [0u64; 16];
    v[..8].copy_from_slice(h);
    v[8..].copy_from_slice(&IV);
    v[12] ^= t0;
    v[13] ^= t1;
    v[14] ^= f0;
    v[15] ^= f1;
    for s in &SIGMA {
        g!(v, 0, 4, 8, 12, m[s[0]], m[s[1]]);
        g!(v, 1, 5, 9, 13, m[s[2]], m[s[3]]);
        g!(v, 2, 6, 10, 14, m[s[4]], m[s[5]]);
        g!(v, 3, 7, 11, 15, m[s[6]], m[s[7]]);
        g!(v, 0, 5, 10, 15, m[s[8]], m[s[9]]);
        g!(v, 1, 6, 11, 12, m[s[10]], m[s[11]]);
        g!(v, 2, 7, 8, 13, m[s[12]], m[s[13]]);
        g!(v, 3, 4, 9, 14, m[s[14]], m[s[15]]);
    }
    for i in 0..8 {
        h[i] ^= v[i] ^ v[i + 8];
    }
}

/// A streaming BLAKE2b state.
#[derive(Clone)]
pub struct Blake2b {
    h: [u64; 8],
    t: [u64; 2],
    f: [u64; 2],
    buf: [u8; 2 * BLOCK],
    buflen: usize,
    last_node: u8,
}

impl Drop for Blake2b {
    fn drop(&mut self) {
        purecrypto::zeroize::Zeroize::zeroize(&mut self.buf[..]);
        for w in self.h.iter_mut() {
            // Volatile-free but harmless: the buffer above is the secret part
            // (key block); the chaining value is wiped as well.
            *w = 0;
        }
        std::hint::black_box(&self.h);
    }
}

impl Blake2b {
    /// A state for an `out_len`-byte digest (1..=64) with an optional key
    /// (0..=64 bytes), salt and personalization (16 bytes each, zero when
    /// absent). Lengths are the caller's responsibility.
    pub fn new(
        out_len: usize,
        key: &[u8],
        salt: Option<&[u8; 16]>,
        personal: Option<&[u8; 16]>,
    ) -> Blake2b {
        debug_assert!((1..=64).contains(&out_len) && key.len() <= 64);
        let mut h = IV;
        h[0] ^= 0x0101_0000 ^ ((key.len() as u64) << 8) ^ out_len as u64;
        if let Some(s) = salt {
            h[4] ^= u64::from_le_bytes(s[..8].try_into().unwrap());
            h[5] ^= u64::from_le_bytes(s[8..].try_into().unwrap());
        }
        if let Some(p) = personal {
            h[6] ^= u64::from_le_bytes(p[..8].try_into().unwrap());
            h[7] ^= u64::from_le_bytes(p[8..].try_into().unwrap());
        }
        let mut st = Blake2b {
            h,
            t: [0; 2],
            f: [0; 2],
            buf: [0; 2 * BLOCK],
            buflen: 0,
            last_node: 0,
        };
        if !key.is_empty() {
            st.buf[..key.len()].copy_from_slice(key);
            st.buflen = BLOCK;
        }
        st
    }

    #[inline(always)]
    fn add_counter(&mut self, n: u64) {
        let (lo, c) = self.t[0].overflowing_add(n);
        self.t[0] = lo;
        self.t[1] = self.t[1].wrapping_add(c as u64);
    }

    #[inline(always)]
    fn compress_block(&mut self, block: &[u8]) {
        self.add_counter(BLOCK as u64);
        compress(
            &mut self.h,
            block,
            self.t[0],
            self.t[1],
            self.f[0],
            self.f[1],
        );
    }

    /// Absorbs `input`.
    pub fn update(&mut self, mut input: &[u8]) {
        while !input.is_empty() {
            let left = self.buflen;
            let fill = 2 * BLOCK - left;
            if input.len() <= fill {
                self.buf[left..left + input.len()].copy_from_slice(input);
                self.buflen += input.len();
                return;
            }
            if left == BLOCK {
                // Steady state: the buffered block is compressed, then every
                // full block but the last one of what follows; that last one
                // becomes the buffered block (in both halves of the buffer,
                // as copying it through the buffer leaves it).
                let n = (input.len() - 1) / BLOCK;
                let first = self.buf;
                self.compress_block(&first[..BLOCK]);
                for c in input[..(n - 1) * BLOCK].as_chunks::<BLOCK>().0 {
                    self.compress_block(c);
                }
                let last = &input[(n - 1) * BLOCK..n * BLOCK];
                self.buf[..BLOCK].copy_from_slice(last);
                self.buf[BLOCK..].copy_from_slice(last);
                input = &input[n * BLOCK..];
                continue;
            }
            self.buf[left..].copy_from_slice(&input[..fill]);
            self.buflen += fill;
            let first = self.buf;
            self.compress_block(&first[..BLOCK]);
            self.buf.copy_within(BLOCK.., 0);
            self.buflen -= BLOCK;
            input = &input[fill..];
        }
    }

    /// Finishes the hash and writes the first `out.len()` (at most 64)
    /// bytes of the chaining value.
    pub fn finalize_into(mut self, out: &mut [u8]) {
        if self.buflen > BLOCK {
            let first = self.buf;
            self.compress_block(&first[..BLOCK]);
            self.buflen -= BLOCK;
            let n = self.buflen;
            self.buf.copy_within(BLOCK..BLOCK + n, 0);
        }
        let n = self.buflen.min(BLOCK);
        self.add_counter(n as u64);
        self.f[0] = u64::MAX;
        if self.last_node != 0 {
            self.f[1] = u64::MAX;
        }
        self.buf[n..BLOCK].fill(0);
        let block = self.buf;
        compress(
            &mut self.h,
            &block[..BLOCK],
            self.t[0],
            self.t[1],
            self.f[0],
            self.f[1],
        );
        let mut full = [0u8; 64];
        for (c, w) in full.as_chunks_mut::<8>().0.iter_mut().zip(self.h) {
            *c = w.to_le_bytes();
        }
        out.copy_from_slice(&full[..out.len()]);
        purecrypto::zeroize::Zeroize::zeroize(&mut full[..]);
    }

    /// The 384-byte serialized state.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut s = vec![0u8; STATE_BYTES];
        for (c, w) in s[..64].as_chunks_mut::<8>().0.iter_mut().zip(self.h) {
            *c = w.to_le_bytes();
        }
        s[64..72].copy_from_slice(&self.t[0].to_le_bytes());
        s[72..80].copy_from_slice(&self.t[1].to_le_bytes());
        s[80..88].copy_from_slice(&self.f[0].to_le_bytes());
        s[88..96].copy_from_slice(&self.f[1].to_le_bytes());
        s[96..352].copy_from_slice(&self.buf);
        s[352..360].copy_from_slice(&(self.buflen as u64).to_le_bytes());
        s[360] = self.last_node;
        s
    }

    /// Reads a serialized state (`s.len()` must be [`STATE_BYTES`]). A
    /// buffered length beyond the buffer, which only a forged state can
    /// hold, is clamped to the buffer size.
    pub fn from_bytes(s: &[u8]) -> Blake2b {
        let w = |i: usize| u64::from_le_bytes(s[i..i + 8].try_into().unwrap());
        let mut h = [0u64; 8];
        for (i, x) in h.iter_mut().enumerate() {
            *x = w(i * 8);
        }
        let mut buf = [0u8; 2 * BLOCK];
        buf.copy_from_slice(&s[96..352]);
        Blake2b {
            h,
            t: [w(64), w(72)],
            f: [w(80), w(88)],
            buf,
            buflen: w(352).min(2 * BLOCK as u64) as usize,
            last_node: s[360],
        }
    }
}

/// One-shot BLAKE2b.
pub fn hash(out: &mut [u8], key: &[u8], data: &[u8]) {
    let mut st = Blake2b::new(out.len(), key, None, None);
    st.update(data);
    st.finalize_into(out);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn rfc7693_abc() {
        // RFC 7693 Appendix A: BLAKE2b-512("abc").
        let mut out = [0u8; 64];
        hash(&mut out, b"", b"abc");
        assert_eq!(
            hex(&out),
            "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d1\
             7d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923"
        );
    }

    #[test]
    fn streaming_matches_one_shot() {
        let data: Vec<u8> = (0..2000u32).map(|i| (i * 7 + 3) as u8).collect();
        for key in [&b""[..], b"0123456789abcdef"] {
            let mut want = [0u8; 48];
            hash(&mut want, key, &data);
            for split in [0, 1, 127, 128, 129, 255, 256, 257, 1000] {
                let mut st = Blake2b::new(48, key, None, None);
                st.update(&data[..split]);
                let st = Blake2b::from_bytes(&st.to_bytes());
                let mut st2 = st.clone();
                st2.update(&data[split..]);
                let mut got = [0u8; 48];
                st2.finalize_into(&mut got);
                assert_eq!(got, want, "split {split}");
            }
        }
    }
}
