//! The global functions: `mt_srand`/`srand`, `mt_rand`/`rand`,
//! `mt_getrandmax`/`getrandmax`, `lcg_value`, `random_int`, `random_bytes`,
//! and the seeded behaviour of `shuffle`, `str_shuffle` and `array_rand`.
//!
//! [`LegacyRandom`] is the per-request state: the global Mt19937 (seeded
//! from the CSPRNG on first use unless `mt_srand()` ran) and the
//! `lcg_value()` generator. `shuffle()`, `str_shuffle()` and `array_rand()`
//! draw from the global Mt19937 with the standard reduction even in
//! `MT_RAND_PHP` mode (test 01_functions/array_rand_mt_rand_php.phpt;
//! observed on PHP 8.5), and match `Randomizer::shuffleArray()`,
//! `shuffleBytes()` and `pickArrayKeys()` on an `Mt19937` with the same seed
//! (test 03_randomizer/compatibility_array_rand.phpt).

use crate::randomizer::{self, PickCaller, PickSource};
use crate::secure::Secure;
use crate::{MSG_MT_RAND_PHP_DEPRECATED, MT_RAND_MAX, Mt19937, MtMode, RandomError};

pub use crate::mt19937::MT_RAND_MAX as RAND_MAX;

/// The per-request random state behind the global functions.
#[derive(Debug, Clone, Default)]
pub struct LegacyRandom {
    mt: Option<Box<Mt19937>>,
    lcg: Option<Lcg>,
}

impl LegacyRandom {
    /// Unseeded state (as at request start).
    pub fn new() -> LegacyRandom {
        LegacyRandom::default()
    }

    /// `mt_srand($seed, $mode)` and `srand()`: `None` seeds from the CSPRNG.
    /// Returns the `E_DEPRECATED` message to emit for `MT_RAND_PHP`
    /// (mode 1 exactly; other values are the standard algorithm, test
    /// mt_srand_unknown_mode.phpt).
    pub fn mt_srand(&mut self, seed: Option<i64>, mode: i64) -> Option<&'static str> {
        let mode = MtMode::from_int(mode);
        let seed = match seed {
            Some(s) => s,
            None => random_seed32(),
        };
        self.mt = Some(Box::new(Mt19937::new(seed, mode)));
        (mode == MtMode::Php).then_some(MSG_MT_RAND_PHP_DEPRECATED)
    }

    /// The global Mt19937, seeded from the CSPRNG on first use.
    pub fn mt(&mut self) -> &mut Mt19937 {
        self.mt
            .get_or_insert_with(|| Box::new(Mt19937::new(random_seed32(), MtMode::Mt19937)))
    }

    /// `mt_rand()` and `rand()` without arguments.
    #[inline]
    pub fn mt_rand(&mut self) -> i64 {
        self.mt().next_int()
    }

    /// `mt_rand($min, $max)`.
    #[inline]
    pub fn mt_rand_range(&mut self, min: i64, max: i64) -> Result<i64, RandomError> {
        self.mt().mt_rand_range(min, max)
    }

    /// `rand($min, $max)` (an inverted range is accepted).
    #[inline]
    pub fn rand_range(&mut self, min: i64, max: i64) -> i64 {
        self.mt().rand_range(min, max)
    }

    /// `mt_getrandmax()` and `getrandmax()`.
    pub fn getrandmax() -> i64 {
        MT_RAND_MAX
    }

    /// `shuffle($array)` on the array's values (the result is a list).
    pub fn shuffle<T>(&mut self, values: &mut [T]) {
        // The MT engine cannot fail.
        let _ = randomizer::shuffle(self.mt(), values);
    }

    /// `str_shuffle($string)`, in place.
    pub fn str_shuffle(&mut self, bytes: &mut [u8]) {
        let _ = randomizer::shuffle(self.mt(), bytes);
    }

    /// `array_rand($array, $num)`: the ordinal positions of the picked
    /// elements in array order (one position for `$num == 1`, which
    /// `array_rand()` returns as a single key).
    pub fn array_rand<S: PickSource + ?Sized>(
        &mut self,
        src: &S,
        num: i64,
    ) -> Result<Vec<usize>, RandomError> {
        randomizer::pick_keys(self.mt(), PickCaller::ArrayRand, src, num)
    }

    /// `lcg_value()`: a float in (0, 1) from the combined linear
    /// congruential generator of L'Ecuyer ("Efficient and portable combined
    /// random number generators", CACM 31(6), 1988), seeded from the CSPRNG.
    /// The caller emits [`crate::MSG_LCG_VALUE_DEPRECATED`] first.
    pub fn lcg_value(&mut self) -> f64 {
        self.lcg.get_or_insert_with(Lcg::seeded).next()
    }
}

fn random_seed32() -> i64 {
    let mut b = [0u8; 4];
    if crate::secure::fill(&mut b).is_err() {
        // No CSPRNG: fall back to the clock, like a time-based seed.
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        return (t ^ (t >> 32)) as u32 as i64;
    }
    u32::from_le_bytes(b) as i64
}

#[derive(Debug, Clone, Copy)]
struct Lcg {
    s1: i64,
    s2: i64,
}

impl Lcg {
    fn seeded() -> Lcg {
        let mut b = [0u8; 8];
        let _ = crate::secure::fill(&mut b);
        let s1 = (u32::from_le_bytes(b[..4].try_into().unwrap()) as i64 % 2_147_483_562) + 1;
        let s2 = (u32::from_le_bytes(b[4..].try_into().unwrap()) as i64 % 2_147_483_398) + 1;
        Lcg { s1, s2 }
    }

    fn next(&mut self) -> f64 {
        // Schrage's method for each component, as in the paper.
        let q = self.s1 / 53668;
        self.s1 = 40014 * (self.s1 - q * 53668) - q * 12211;
        if self.s1 < 0 {
            self.s1 += 2_147_483_563;
        }
        let q = self.s2 / 52774;
        self.s2 = 40692 * (self.s2 - q * 52774) - q * 3791;
        if self.s2 < 0 {
            self.s2 += 2_147_483_399;
        }
        let mut z = self.s1 - self.s2;
        if z < 1 {
            z += 2_147_483_562;
        }
        z as f64 * 4.656613e-10
    }
}

/// `random_int($min, $max)` from the CSPRNG.
pub fn random_int(min: i64, max: i64) -> Result<i64, RandomError> {
    if min > max {
        return Err(RandomError::value(
            "random_int(): Argument #1 ($min) must be less than or equal to argument #2 ($max)",
        ));
    }
    randomizer::range(&mut Secure, min, max)
}

/// `random_bytes($length)` from the CSPRNG (`$length` 0 is a `ValueError`,
/// test 01_functions/random_bytes_error.phpt).
pub fn random_bytes(length: i64) -> Result<Vec<u8>, RandomError> {
    if length < 1 {
        return Err(RandomError::value(
            "random_bytes(): Argument #1 ($length) must be greater than 0",
        ));
    }
    let mut out = vec![0u8; length as usize];
    crate::secure::fill(&mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lcg_in_range() {
        let mut l = LegacyRandom::new();
        for _ in 0..10_000 {
            let v = l.lcg_value();
            assert!(v > 0.0 && v < 1.0);
        }
    }

    // tests 01_functions/random_int*.phpt, random_bytes*.phpt
    #[test]
    fn csprng() {
        assert_eq!(random_int(42, 42).unwrap(), 42);
        assert!(random_int(-1000, -1).unwrap() < 0);
        random_int(i64::MIN, i64::MAX).unwrap();
        assert!(random_int(42, 0).is_err());
        assert_eq!(random_bytes(257).unwrap().len(), 257);
        assert_eq!(
            random_bytes(0).unwrap_err(),
            RandomError::value("random_bytes(): Argument #1 ($length) must be greater than 0")
        );
    }

    // test 01_functions/array_rand_mt_rand_php.phpt
    #[test]
    fn array_rand_php_mode() {
        let mut l = LegacyRandom::new();
        assert_eq!(l.mt_srand(Some(1), 1), Some(MSG_MT_RAND_PHP_DEPRECATED));
        let keys: Vec<usize> = (0..3)
            .map(|_| l.array_rand(&3usize, 1).unwrap()[0])
            .collect();
        assert_eq!(keys, [0, 1, 0]);
    }
}
