//! `mt_rand()`, `mt_srand()`, `rand()` and `srand()`: the Mersenne Twister
//! MT19937 generator.
//!
//! The generator is the published MT19937 (Matsumoto and Nishimura, 1998,
//! "Mersenne Twister: a 623-dimensionally equidistributed uniform
//! pseudo-random number generator"), seeded with its `init_genrand`
//! procedure from the low 32 bits of the seed. `mt_rand()` without
//! arguments returns the 32-bit output shifted right by one.
//!
//! `MT_RAND_PHP` selects the variant PHP used before 7.1, whose twist takes
//! the low bit of the wrong state word (described in the rng_fixes RFC,
//! wiki.php.net/rfc/rng_fixes), and its range scaling by multiplication.
//! Range selection otherwise rejects biased draws. Sequences checked
//! against ext/random/tests/01_functions/mt_rand_value.phpt, bug75514.phpt,
//! mt_srand_unknown_mode.phpt and observations on PHP 8.5
//! (tests/fixtures/mt_rand.txt).

use crate::error::StdError;

const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908_b0df;

/// Which MT19937 flavour the generator follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MtMode {
    /// `MT_RAND_MT19937`: the standard algorithm.
    Mt19937,
    /// `MT_RAND_PHP`: PHP's historical variant (deprecated since 8.3).
    Php,
}

impl MtMode {
    /// The mode for `mt_srand()`'s `$mode` argument: 1 (after truncation to
    /// 8 bits) is `MT_RAND_PHP`, anything else the standard algorithm
    /// (test mt_srand_unknown_mode.phpt).
    pub fn from_int(mode: i64) -> MtMode {
        if mode as u8 == 1 {
            MtMode::Php
        } else {
            MtMode::Mt19937
        }
    }
}

/// The largest value `mt_rand()` returns without arguments
/// (`mt_getrandmax()`).
pub const MT_RAND_MAX: i64 = 0x7fff_ffff;

/// An MT19937 generator state.
#[derive(Clone)]
pub struct Mt19937 {
    state: [u32; N],
    index: usize,
    mode: MtMode,
}

impl std::fmt::Debug for Mt19937 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mt19937")
            .field("index", &self.index)
            .field("mode", &self.mode)
            .finish_non_exhaustive()
    }
}

impl Mt19937 {
    /// A generator seeded as by `mt_srand($seed, $mode)`.
    pub fn new(seed: i64, mode: MtMode) -> Mt19937 {
        let mut state = [0u32; N];
        state[0] = seed as u32;
        for i in 1..N {
            let prev = state[i - 1];
            state[i] = 1_812_433_253u32
                .wrapping_mul(prev ^ (prev >> 30))
                .wrapping_add(i as u32);
        }
        let mut g = Mt19937 {
            state,
            index: N,
            mode,
        };
        g.reload();
        g
    }

    /// The seeding mode.
    pub fn mode(&self) -> MtMode {
        self.mode
    }

    fn reload(&mut self) {
        let twist = |mode: MtMode, u: u32, v: u32| -> u32 {
            let mix = (u & 0x8000_0000) | (v & 0x7fff_ffff);
            let low = match mode {
                MtMode::Mt19937 => v & 1,
                MtMode::Php => u & 1,
            };
            (mix >> 1) ^ if low != 0 { MATRIX_A } else { 0 }
        };
        let s = &mut self.state;
        for i in 0..N {
            let next = s[(i + 1) % N];
            s[i] = s[(i + M) % N] ^ twist(self.mode, s[i], next);
        }
        self.index = 0;
    }

    /// The next tempered 32-bit output.
    pub fn next_u32(&mut self) -> u32 {
        if self.index >= N {
            self.reload();
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    /// `mt_rand()` without arguments: 0 to [`MT_RAND_MAX`].
    pub fn next_int(&mut self) -> i64 {
        (self.next_u32() >> 1) as i64
    }

    /// A uniform value in `0..=umax` using rejection of biased draws.
    fn range32(&mut self, umax: u32) -> u32 {
        let mut r = self.next_u32();
        if umax == u32::MAX {
            return r;
        }
        let n = umax + 1;
        if n & umax == 0 {
            return r & umax;
        }
        let limit = u32::MAX - (u32::MAX % n) - 1;
        while r > limit {
            r = self.next_u32();
        }
        r % n
    }

    fn range64(&mut self, umax: u64) -> u64 {
        // Observed on PHP 8.5: the first draw is the low half.
        let next = |g: &mut Self| {
            let lo = g.next_u32() as u64;
            lo | ((g.next_u32() as u64) << 32)
        };
        let mut r = next(self);
        if umax == u64::MAX {
            return r;
        }
        let n = umax + 1;
        if n & umax == 0 {
            return r & umax;
        }
        let limit = u64::MAX - (u64::MAX % n) - 1;
        while r > limit {
            r = next(self);
        }
        r % n
    }

    /// A uniform value in `min..=max` (`min <= max`), as `mt_rand($min,
    /// $max)` draws it in the standard mode.
    pub fn range(&mut self, min: i64, max: i64) -> i64 {
        let umax = (max as u64).wrapping_sub(min as u64);
        let r = if umax > u32::MAX as u64 {
            self.range64(umax)
        } else {
            self.range32(umax as u32) as u64
        };
        (min as u64).wrapping_add(r) as i64
    }

    /// `mt_rand($min, $max)`.
    pub fn mt_rand_range(&mut self, min: i64, max: i64) -> Result<i64, StdError> {
        if max < min {
            return Err(StdError::ValueError(
                "mt_rand(): Argument #2 ($max) must be greater than or equal to argument #1 ($min)"
                    .into(),
            ));
        }
        Ok(self.common_range(min, max))
    }

    /// `rand($min, $max)`: like `mt_rand()`, but an inverted range is
    /// accepted (test rand_inverted_order.phpt).
    pub fn rand_range(&mut self, min: i64, max: i64) -> i64 {
        if max < min {
            return self.common_range(max, min);
        }
        self.common_range(min, max)
    }

    fn common_range(&mut self, min: i64, max: i64) -> i64 {
        match self.mode {
            MtMode::Mt19937 => self.range(min, max),
            MtMode::Php => {
                // The pre-7.1 scaling: n * (max - min + 1) / (MT_RAND_MAX + 1).
                let n = self.next_int() as f64;
                let span = (max as f64) - (min as f64) + 1.0;
                // Out-of-range products wrap like PHP's float to int casts.
                min.wrapping_add(crate::printf::float_to_int(
                    span * (n / (MT_RAND_MAX as f64 + 1.0)),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // test ext/random/tests/01_functions/mt_rand_value.phpt
    #[test]
    fn mt_rand_value() {
        let mut g = Mt19937::new(12345678, MtMode::Php);
        let want = [
            1614640687, 1711027313, 857485497, 688176834, 1386682158, 412773096, 813703253,
            898651287, 2087374214, 1382556330, 1640700129, 1863374167, 1324097651, 1923803667,
            676334965, 853386222,
        ];
        for w in want {
            assert_eq!(g.next_int(), w);
        }
        let mut x = 0;
        for _ in 0..1024 {
            x ^= g.next_int();
        }
        assert_eq!(x, 1571178311);

        let mut g = Mt19937::new(12345678, MtMode::Mt19937);
        let want = [
            527860569, 1711027313, 1280820687, 688176834, 770499160, 412773096, 813703253,
            898651287, 52508912, 757323740, 511765911, 274407457, 833082629, 1923803667,
            1461450755, 1301698200,
        ];
        for w in want {
            assert_eq!(g.next_int(), w);
        }
        let mut x = 0;
        for _ in 0..1024 {
            x ^= g.next_int();
        }
        assert_eq!(x, 1612214270);
    }

    // tests bug75514.phpt and mt_srand_unknown_mode.phpt
    #[test]
    fn ranges_and_modes() {
        let mut g = Mt19937::new(0, MtMode::Php);
        assert_eq!(g.mt_rand_range(0, 999_999_999).unwrap(), 448865905);
        assert_eq!(g.mt_rand_range(0, 999).unwrap(), 592);
        assert_eq!(Mt19937::new(1, MtMode::from_int(0)).next_int(), 895547922);
        assert_eq!(Mt19937::new(1, MtMode::from_int(1)).next_int(), 1244335972);
        assert_eq!(Mt19937::new(1, MtMode::from_int(2)).next_int(), 895547922);
        assert_eq!(Mt19937::new(1, MtMode::from_int(256)).next_int(), 895547922);
        assert!(g.mt_rand_range(10, 1).is_err());
    }
}
