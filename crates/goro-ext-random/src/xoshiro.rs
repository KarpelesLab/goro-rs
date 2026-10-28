//! `Random\Engine\Xoshiro256StarStar`: xoshiro256** (Blackman and Vigna,
//! "Scrambled Linear Pseudorandom Number Generators", 2018, and the
//! reference description at prng.di.unimi.it).
//!
//! `generate()` returns the 64-bit output as 8 little-endian bytes. An int
//! seed fills the four state words with SplitMix64 (the seeding the
//! xoshiro authors recommend); a 32-byte string seed is the four words,
//! each little-endian, and must not be all zero. `jump()` and `jumpLong()`
//! advance by 2^128 and 2^192 steps with the published jump polynomials.
//! Observed on PHP 8.5 and checked against tests
//! 02_engine/xoshiro256starstar_*.phpt.

use crate::engine::{Engine, Generated};
use crate::state::{StateItem, StateValue, parse_words};
use crate::{RandomError, invalid_serialization};

/// The jump polynomial for 2^128 steps (xoshiro256** reference).
const JUMP: [u64; 4] = [
    0x180e_c6d3_3cfd_0aba,
    0xd5a6_1266_f0c9_392c,
    0xa958_2618_e03f_c9aa,
    0x39ab_dc45_29b1_661c,
];
/// The jump polynomial for 2^192 steps (xoshiro256** reference).
const LONG_JUMP: [u64; 4] = [
    0x76e1_5d3e_fefd_cbbf,
    0xc500_4e44_1c52_2fb3,
    0x7771_0069_854e_e241,
    0x3910_9bb0_2acb_e635,
];

/// A xoshiro256** state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Xoshiro256StarStar {
    s: [u64; 4],
}

fn splitmix64(x: &mut u64) -> u64 {
    *x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *x;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

impl Xoshiro256StarStar {
    /// `new Xoshiro256StarStar($seed)` with an int seed.
    pub fn from_int(seed: i64) -> Xoshiro256StarStar {
        let mut x = seed as u64;
        let s = [
            splitmix64(&mut x),
            splitmix64(&mut x),
            splitmix64(&mut x),
            splitmix64(&mut x),
        ];
        Xoshiro256StarStar { s }
    }

    /// The four state words, as a 32-byte seed (or a deserialized state)
    /// gives them.
    pub fn from_words(s: [u64; 4]) -> Xoshiro256StarStar {
        Xoshiro256StarStar { s }
    }

    /// `new Xoshiro256StarStar()`: a seed from the CSPRNG.
    pub fn new_random() -> Result<Xoshiro256StarStar, RandomError> {
        loop {
            let mut b = [0u8; 32];
            crate::secure::fill(&mut b)?;
            if let Ok(e) = Self::from_string(&b) {
                return Ok(e);
            }
        }
    }

    /// `new Xoshiro256StarStar($seed)` with a string seed: it must be 32
    /// bytes, not all NUL (test 02_engine/xoshiro256starstar_seed.phpt).
    pub fn from_string(seed: &[u8]) -> Result<Xoshiro256StarStar, RandomError> {
        if seed.len() != 32 {
            return Err(RandomError::value(
                "Random\\Engine\\Xoshiro256StarStar::__construct(): Argument #1 ($seed) must be a 32 byte (256 bit) string",
            ));
        }
        if seed.iter().all(|&b| b == 0) {
            return Err(RandomError::value(
                "Random\\Engine\\Xoshiro256StarStar::__construct(): Argument #1 ($seed) must not consist entirely of NUL bytes",
            ));
        }
        let w = |i: usize| u64::from_le_bytes(seed[i * 8..i * 8 + 8].try_into().unwrap());
        Ok(Xoshiro256StarStar {
            s: [w(0), w(1), w(2), w(3)],
        })
    }

    /// The next 64-bit output.
    #[inline(always)]
    pub fn next_u64(&mut self) -> u64 {
        let s = &mut self.s;
        let result = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    fn jump_with(&mut self, poly: &[u64; 4]) {
        let mut acc = [0u64; 4];
        for &word in poly {
            for b in 0..64 {
                if word & (1u64 << b) != 0 {
                    for (a, s) in acc.iter_mut().zip(self.s) {
                        *a ^= s;
                    }
                }
                self.next_u64();
            }
        }
        self.s = acc;
    }

    /// `jump()`: 2^128 steps ahead.
    pub fn jump(&mut self) {
        self.jump_with(&JUMP);
    }

    /// `jumpLong()`: 2^192 steps ahead.
    pub fn jump_long(&mut self) {
        self.jump_with(&LONG_JUMP);
    }

    /// `__serialize()[1]`: the four words as the hex of their little-endian
    /// bytes (test 02_engine/xoshiro256starstar_serialize.phpt).
    pub fn state(&self) -> Vec<StateItem> {
        self.s
            .iter()
            .map(|w| StateItem::hex(&w.to_le_bytes()))
            .collect()
    }

    /// `__unserialize()`'s state array: exactly four strings of 16 hex
    /// digits (an all-zero state is accepted, observed on PHP 8.5).
    pub fn from_state(items: &[StateValue<'_>]) -> Result<Xoshiro256StarStar, RandomError> {
        let s =
            parse_words::<4>(items).ok_or_else(|| invalid_serialization(crate::class::XOSHIRO))?;
        Ok(Xoshiro256StarStar { s })
    }
}

impl Engine for Xoshiro256StarStar {
    #[inline(always)]
    fn generate(&mut self) -> Result<Generated, RandomError> {
        Ok(Generated::u64(self.next_u64()))
    }
    fn fill_bytes(&mut self, out: &mut [u8]) -> Option<Result<(), RandomError>> {
        crate::engine::fill_words::<8>(out, || self.next_u64().to_le_bytes());
        Some(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(v: u64) -> String {
        v.to_le_bytes().iter().map(|b| format!("{b:02x}")).collect()
    }

    // tests 02_engine/xoshiro256starstar_value.phpt, _seed.phpt, _serialize.phpt
    #[test]
    fn reference_values() {
        let mut e = Xoshiro256StarStar::from_int(1234);
        let st: Vec<StateItem> = [
            "db1c182f1bf60cbb",
            "2465f04d36a1c797",
            "da25c09be4fabe33",
            "33a0d052f241624e",
        ]
        .iter()
        .map(|s| StateItem::Hex(s.to_string()))
        .collect();
        assert_eq!(e.state(), st);
        for _ in 0..10_000 {
            e.next_u64();
        }
        e.jump();
        e.jump_long();
        assert_eq!(hex(e.next_u64()), "1f197e9ca7969123");

        let seed: Vec<u8> = (0..32).map(|i| (i % 8) as u8 + 1).collect();
        let mut e = Xoshiro256StarStar::from_string(&seed).unwrap();
        assert_eq!(e.state()[0], StateItem::Hex("0102030405060708".into()));
        for _ in 0..1000 {
            e.next_u64();
        }
        assert_eq!(hex(e.next_u64()), "90a025df9300cfd1");
        assert!(Xoshiro256StarStar::from_string(b"foobar").is_err());
        assert!(Xoshiro256StarStar::from_string(&[0; 32]).is_err());
    }
}
