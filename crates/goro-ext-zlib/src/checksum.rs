//! Adler-32 (RFC 1950 section 8) and CRC-32 (RFC 1952 section 8) running
//! checksums. CRC-32 is goro-ext-hash's `crc32b` (carry-less multiply
//! folding where available); Adler-32 is computed here sixteen bytes at a
//! time, which the compiler vectorises.

use goro_ext_hash::Hasher;

const MOD: u32 = 65521;
/// Largest n with 255n(n+1)/2 + (n+1)(MOD-1) < 2^32 (RFC 1950's NMAX),
/// rounded down to whole 16-byte blocks.
const NMAX: usize = 5552 / 16 * 16;

#[derive(Clone, Copy)]
pub struct Adler32(u32);

impl Adler32 {
    pub fn new() -> Self {
        Adler32(1)
    }

    pub fn update(&mut self, data: &[u8]) {
        let (mut s1, mut s2) = (self.0 & 0xffff, self.0 >> 16);
        for chunk in data.chunks(NMAX) {
            let (blocks, rest) = chunk.as_chunks::<16>();
            for b in blocks {
                // Sixteen steps of s1 += x; s2 += s1 at once.
                s2 += 16 * s1;
                let mut sum = 0u32;
                let mut weighted = 0u32;
                for (i, &x) in b.iter().enumerate() {
                    sum += x as u32;
                    weighted += (16 - i as u32) * x as u32;
                }
                s1 += sum;
                s2 += weighted;
            }
            for &x in rest {
                s1 += x as u32;
                s2 += s1;
            }
            s1 %= MOD;
            s2 %= MOD;
        }
        self.0 = (s2 << 16) | s1;
    }

    pub fn value(&self) -> u32 {
        self.0
    }
}

impl Default for Adler32 {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct Crc32(Hasher);

impl Crc32 {
    pub fn new() -> Self {
        Crc32(Hasher::plain(
            goro_ext_hash::lookup(b"crc32b").expect("crc32b"),
        ))
    }

    pub fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    pub fn value(&self) -> u32 {
        let d = self.0.clone().finish();
        u32::from_be_bytes([d[0], d[1], d[2], d[3]])
    }
}

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

/// Adler-32 of `data` alone.
pub fn adler32(data: &[u8]) -> u32 {
    let mut a = Adler32::new();
    a.update(data);
    a.value()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slow(data: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for &x in data {
            a = (a + x as u32) % MOD;
            b = (b + a) % MOD;
        }
        (b << 16) | a
    }

    #[test]
    fn adler_matches_definition() {
        // RFC 1950 section 9 style check: "Wikipedia" is 0x11E60398.
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        let data: Vec<u8> = (0..100_000u32).map(|i| (i * 7 + i / 255) as u8).collect();
        assert_eq!(adler32(&data), slow(&data));
        let ff = vec![0xffu8; 70_000];
        assert_eq!(adler32(&ff), slow(&ff));
        let mut a = Adler32::new();
        for c in ff.chunks(333) {
            a.update(c);
        }
        assert_eq!(a.value(), slow(&ff));
    }
}
