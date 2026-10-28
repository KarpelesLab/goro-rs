//! `Random\Engine\PcgOneseq128XslRr64`: PCG with a 128-bit LCG state, one
//! fixed stream and the XSL RR output function (O'Neill, "PCG: A Family of
//! Simple Fast Space-Efficient Statistically Good Algorithms for Random
//! Number Generation", 2014; the PHP manual and the Random Extension 5.x
//! RFC name the variant).
//!
//! Each `generate()` steps the state, then outputs the XSL RR of the new
//! state as 8 little-endian bytes. Seeding follows PCG's `srandom`: from
//! state 0, step, add the seed, step. An int seed is the 128-bit value
//! with the int's 64 bits low; a 16-byte string seed holds the high then
//! the low half, each little-endian. Observed on PHP 8.5 and checked
//! against tests 02_engine/pcgoneseq128xslrr64_*.phpt.

use crate::engine::{Engine, Generated};
use crate::state::{StateItem, StateValue, parse_words};
use crate::{RandomError, invalid_serialization};

/// PCG's default 128-bit multiplier.
const MUL: u128 = (2549297995355413924u128 << 64) | 4865540595714422341;
/// PCG's default 128-bit increment.
const INC: u128 = (6364136223846793005u128 << 64) | 1442695040888963407;

/// A PcgOneseq128XslRr64 state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcgOneseq128XslRr64 {
    state: u128,
}

impl PcgOneseq128XslRr64 {
    fn seeded(seed: u128) -> PcgOneseq128XslRr64 {
        let mut s = 0u128.wrapping_mul(MUL).wrapping_add(INC);
        s = s.wrapping_add(seed);
        s = s.wrapping_mul(MUL).wrapping_add(INC);
        PcgOneseq128XslRr64 { state: s }
    }

    /// `new PcgOneseq128XslRr64($seed)` with an int seed.
    pub fn from_int(seed: i64) -> PcgOneseq128XslRr64 {
        Self::seeded(seed as u64 as u128)
    }

    /// `new PcgOneseq128XslRr64($seed)` with a 16-byte string seed.
    pub fn from_seed_bytes(seed: &[u8; 16]) -> PcgOneseq128XslRr64 {
        let hi = u64::from_le_bytes(seed[..8].try_into().unwrap());
        let lo = u64::from_le_bytes(seed[8..].try_into().unwrap());
        Self::seeded(((hi as u128) << 64) | lo as u128)
    }

    /// `new PcgOneseq128XslRr64()`: a seed from the CSPRNG.
    pub fn new_random() -> Result<PcgOneseq128XslRr64, RandomError> {
        let mut b = [0u8; 16];
        crate::secure::fill(&mut b)?;
        Ok(Self::from_seed_bytes(&b))
    }

    /// `new PcgOneseq128XslRr64($seed)` with a string seed of any length:
    /// anything but 16 bytes is a `ValueError` (test
    /// 02_engine/pcgoneseq128xslrr64_seed.phpt).
    pub fn from_string(seed: &[u8]) -> Result<PcgOneseq128XslRr64, RandomError> {
        match <&[u8; 16]>::try_from(seed) {
            Ok(b) => Ok(Self::from_seed_bytes(b)),
            Err(_) => Err(RandomError::value(
                "Random\\Engine\\PcgOneseq128XslRr64::__construct(): Argument #1 ($seed) must be a 16 byte (128 bit) string",
            )),
        }
    }

    /// The next 64-bit output.
    #[inline(always)]
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(MUL).wrapping_add(INC);
        let hi = (self.state >> 64) as u64;
        let lo = self.state as u64;
        (hi ^ lo).rotate_right((hi >> 58) as u32)
    }

    /// `jump($advance)`: moves the state `advance` steps ahead, by the usual
    /// LCG jump-ahead (square-and-multiply on the affine step).
    pub fn jump(&mut self, advance: i64) -> Result<(), RandomError> {
        if advance < 0 {
            return Err(RandomError::value(
                "Random\\Engine\\PcgOneseq128XslRr64::jump(): Argument #1 ($advance) must be greater than or equal to 0",
            ));
        }
        let mut delta = advance as u64;
        let (mut acc_mul, mut acc_add) = (1u128, 0u128);
        let (mut cur_mul, mut cur_add) = (MUL, INC);
        while delta > 0 {
            if delta & 1 != 0 {
                acc_mul = acc_mul.wrapping_mul(cur_mul);
                acc_add = acc_add.wrapping_mul(cur_mul).wrapping_add(cur_add);
            }
            cur_add = cur_mul.wrapping_add(1).wrapping_mul(cur_add);
            cur_mul = cur_mul.wrapping_mul(cur_mul);
            delta >>= 1;
        }
        self.state = acc_mul.wrapping_mul(self.state).wrapping_add(acc_add);
        Ok(())
    }

    /// `__serialize()[1]`: the high then the low 64 bits, each as the hex
    /// of its little-endian bytes (test 02_engine/pcgoneseq128xslrr64_serialize.phpt).
    pub fn state(&self) -> Vec<StateItem> {
        let hi = (self.state >> 64) as u64;
        let lo = self.state as u64;
        vec![
            StateItem::hex(&hi.to_le_bytes()),
            StateItem::hex(&lo.to_le_bytes()),
        ]
    }

    /// `__unserialize()`'s state array: exactly two strings of 16 hex digits.
    pub fn from_state(items: &[StateValue<'_>]) -> Result<PcgOneseq128XslRr64, RandomError> {
        let [hi, lo] =
            parse_words::<2>(items).ok_or_else(|| invalid_serialization(crate::class::PCG))?;
        Ok(PcgOneseq128XslRr64 {
            state: ((hi as u128) << 64) | lo as u128,
        })
    }
}

impl Engine for PcgOneseq128XslRr64 {
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

    // tests 02_engine/pcgoneseq128xslrr64_value.phpt, _seed.phpt, _serialize.phpt
    #[test]
    fn reference_values() {
        let mut e = PcgOneseq128XslRr64::from_int(1234);
        assert_eq!(
            e.state(),
            vec![
                StateItem::Hex("c6d571c37c41a8d1".into()),
                StateItem::Hex("345e7e82265d6e27".into())
            ]
        );
        for _ in 0..10_000 {
            e.next_u64();
        }
        e.jump(1234567).unwrap();
        assert_eq!(hex(e.next_u64()), "b88e2a0f23720a1d");

        let mut e =
            PcgOneseq128XslRr64::from_string(&[1, 2, 3, 4, 5, 6, 7, 8, 1, 2, 3, 4, 5, 6, 7, 8])
                .unwrap();
        assert_eq!(
            e.state(),
            vec![
                StateItem::Hex("7afbdfd5830d8250".into()),
                StateItem::Hex("dfc50b6959b3bafc".into())
            ]
        );
        for _ in 0..1000 {
            e.next_u64();
        }
        assert_eq!(hex(e.next_u64()), "c42016cd9005ef2e");
        assert!(PcgOneseq128XslRr64::from_string(b"foobar").is_err());
        assert!(e.jump(-1).is_err());
    }
}
