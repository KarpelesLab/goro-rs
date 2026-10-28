//! `Random\Engine\Mt19937` and the generator behind `mt_rand()`,
//! `mt_srand()`, `rand()` and `srand()`.
//!
//! The generator is the published MT19937 (Matsumoto and Nishimura, 1998,
//! "Mersenne Twister: a 623-dimensionally equidistributed uniform
//! pseudo-random number generator"), seeded with its `init_genrand`
//! procedure from the low 32 bits of the seed. `generate()` returns the
//! tempered 32-bit output little-endian; `mt_rand()` without arguments
//! returns it shifted right by one.
//!
//! `MT_RAND_PHP` selects the variant PHP used before 7.1, whose twist takes
//! the low bit of the wrong state word (described in the rng_fixes RFC,
//! wiki.php.net/rfc/rng_fixes), and its range scaling by multiplication.
//! Range selection otherwise rejects biased draws (see
//! [`crate::randomizer::range`]). Sequences checked against
//! ext/random/tests/01_functions/mt_rand_value.phpt, bug75514.phpt,
//! mt_srand_unknown_mode.phpt, 02_engine/mt19937_*.phpt and observations on
//! PHP 8.5 (tests/fixtures/).

use crate::engine::{Engine, Generated};
use crate::state::{StateItem, StateValue, parse_hex};
use crate::{RandomError, invalid_serialization};

const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908_b0df;

/// Which MT19937 flavour the generator follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MtMode {
    /// `MT_RAND_MT19937` (0): the standard algorithm.
    Mt19937,
    /// `MT_RAND_PHP` (1): PHP's historical variant (deprecated since 8.3).
    Php,
}

impl MtMode {
    /// The mode for `mt_srand()`'s `$mode` argument: 1 is `MT_RAND_PHP`,
    /// anything else the standard algorithm (test mt_srand_unknown_mode.phpt;
    /// observed on PHP 8.5: 257 and -255 are also the standard algorithm).
    pub fn from_int(mode: i64) -> MtMode {
        if mode == 1 {
            MtMode::Php
        } else {
            MtMode::Mt19937
        }
    }

    /// The constant's value.
    pub fn to_int(self) -> i64 {
        match self {
            MtMode::Mt19937 => 0,
            MtMode::Php => 1,
        }
    }
}

/// The largest value `mt_rand()` returns without arguments
/// (`mt_getrandmax()`, `getrandmax()`).
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

    /// A generator seeded from the CSPRNG (`new Mt19937()`, and `mt_rand()`
    /// before any `mt_srand()`).
    pub fn new_random(mode: MtMode) -> Result<Mt19937, RandomError> {
        let mut b = [0u8; 4];
        crate::secure::fill(&mut b)?;
        Ok(Mt19937::new(u32::from_le_bytes(b) as i64, mode))
    }

    /// `new Random\Engine\Mt19937($seed, $mode)`: `None` seeds from the
    /// CSPRNG. The mode must be exactly `MT_RAND_MT19937` or `MT_RAND_PHP`
    /// (test 02_engine/mt19937_error.phpt); for `MT_RAND_PHP` the caller
    /// emits [`crate::MSG_MT_RAND_PHP_DEPRECATED`] as `E_DEPRECATED` first.
    pub fn construct(seed: Option<i64>, mode: i64) -> Result<Mt19937, RandomError> {
        let mode = match mode {
            0 => MtMode::Mt19937,
            1 => MtMode::Php,
            _ => {
                return Err(RandomError::value(
                    "Random\\Engine\\Mt19937::__construct(): Argument #2 ($mode) must be either MT_RAND_MT19937 or MT_RAND_PHP",
                ));
            }
        };
        match seed {
            Some(s) => Ok(Mt19937::new(s, mode)),
            None => Mt19937::new_random(mode),
        }
    }

    /// The seeding mode.
    pub fn mode(&self) -> MtMode {
        self.mode
    }

    #[inline(always)]
    fn twist(php: bool, u: u32, v: u32) -> u32 {
        let mix = (u & 0x8000_0000) | (v & 0x7fff_ffff);
        let low = if php { u } else { v } & 1;
        (mix >> 1) ^ (low.wrapping_neg() & MATRIX_A)
    }

    fn reload_mode<const PHP: bool>(s: &mut [u32; N]) {
        for i in 0..N - M {
            s[i] = s[i + M] ^ Self::twist(PHP, s[i], s[i + 1]);
        }
        for i in N - M..N - 1 {
            s[i] = s[i + M - N] ^ Self::twist(PHP, s[i], s[i + 1]);
        }
        s[N - 1] = s[M - 1] ^ Self::twist(PHP, s[N - 1], s[0]);
    }

    #[cold]
    fn reload(&mut self) {
        match self.mode {
            MtMode::Mt19937 => Self::reload_mode::<false>(&mut self.state),
            MtMode::Php => Self::reload_mode::<true>(&mut self.state),
        }
        self.index = 0;
    }

    /// The next tempered 32-bit output (`generate()`'s bytes, little-endian).
    #[inline]
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
    #[inline]
    pub fn next_int(&mut self) -> i64 {
        (self.next_u32() >> 1) as i64
    }

    /// A uniform value in `min..=max` (`min <= max`) as `mt_rand($min,
    /// $max)` draws it: the standard reduction, or in `MT_RAND_PHP` mode the
    /// pre-7.1 scaling `min + (max - min + 1) * (n / (MT_RAND_MAX + 1))`
    /// computed in floating point.
    #[inline]
    pub fn range(&mut self, min: i64, max: i64) -> i64 {
        match self.mode {
            MtMode::Mt19937 => crate::randomizer::range_unchecked(self, min, max),
            MtMode::Php => self.php_scaled_range(min, max),
        }
    }

    /// The `MT_RAND_PHP` range scaling.
    pub fn php_scaled_range(&mut self, min: i64, max: i64) -> i64 {
        let n = self.next_int() as f64;
        let span = (max as f64) - (min as f64) + 1.0;
        // Out-of-range products wrap like PHP's float to int casts.
        min.wrapping_add(float_to_int(span * (n / (MT_RAND_MAX as f64 + 1.0))))
    }

    /// `mt_rand($min, $max)`.
    pub fn mt_rand_range(&mut self, min: i64, max: i64) -> Result<i64, RandomError> {
        if max < min {
            return Err(RandomError::value(
                "mt_rand(): Argument #2 ($max) must be greater than or equal to argument #1 ($min)",
            ));
        }
        Ok(self.range(min, max))
    }

    /// `rand($min, $max)`: like `mt_rand()`, but an inverted range is
    /// accepted (test rand_inverted_order.phpt).
    pub fn rand_range(&mut self, min: i64, max: i64) -> i64 {
        if max < min {
            return self.range(max, min);
        }
        self.range(min, max)
    }

    /// `__serialize()[1]` and `__debugInfo()["__states"]`: the 624 state
    /// words as 8 hex digits of their little-endian bytes, then the index of
    /// the next word (0 to 624) and the mode (test
    /// 02_engine/mt19937_serialize.phpt).
    pub fn state(&self) -> Vec<StateItem> {
        let mut v: Vec<StateItem> = self
            .state
            .iter()
            .map(|w| StateItem::hex(&w.to_le_bytes()))
            .collect();
        v.push(StateItem::Int(self.index as i64));
        v.push(StateItem::Int(self.mode.to_int()));
        v
    }

    /// `__unserialize()`'s state array (see [`crate::state`]): 624 strings
    /// of 8 hex digits, the index (an int, 0 to 624) and the mode (an int,
    /// 0 or 1); anything else is "Invalid serialization data for
    /// Random\Engine\Mt19937 object" (test 02_engine/all_serialize_error.phpt).
    pub fn from_state(items: &[StateValue<'_>]) -> Result<Mt19937, RandomError> {
        let err = || invalid_serialization(crate::class::MT19937);
        if items.len() != N + 2 {
            return Err(err());
        }
        let mut state = [0u32; N];
        for (w, item) in state.iter_mut().zip(items) {
            let StateValue::Str(s) = item else {
                return Err(err());
            };
            let b: [u8; 4] = parse_hex(s).ok_or_else(err)?;
            *w = u32::from_le_bytes(b);
        }
        let index = match items[N] {
            StateValue::Int(i) if (0..=N as i64).contains(&i) => i as usize,
            _ => return Err(err()),
        };
        let mode = match items[N + 1] {
            StateValue::Int(0) => MtMode::Mt19937,
            StateValue::Int(1) => MtMode::Php,
            _ => return Err(err()),
        };
        Ok(Mt19937 { state, index, mode })
    }
}

impl Engine for Mt19937 {
    #[inline(always)]
    fn generate(&mut self) -> Result<Generated, RandomError> {
        Ok(Generated::u32(self.next_u32()))
    }
    fn mt_php_scaling(&self) -> bool {
        self.mode == MtMode::Php
    }
    fn fill_bytes(&mut self, out: &mut [u8]) -> Option<Result<(), RandomError>> {
        crate::engine::fill_words::<4>(out, || self.next_u32().to_le_bytes());
        Some(Ok(()))
    }
    fn as_mt19937(&mut self) -> Option<&mut Mt19937> {
        Some(self)
    }
}

/// PHP's float to int conversion for the scaled range: truncation, with
/// out-of-range values reduced modulo 2^64 and non-finite values 0.
pub(crate) fn float_to_int(f: f64) -> i64 {
    if !f.is_finite() {
        return 0;
    }
    let t = f.trunc();
    let two63 = (1u64 << 63) as f64;
    if (-two63..two63).contains(&t) {
        return t as i64;
    }
    // Every float this large is an integer.
    let m = t % 18_446_744_073_709_551_616.0;
    let m = if m < 0.0 {
        m + 18_446_744_073_709_551_616.0
    } else {
        m
    };
    (m as u64) as i64
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
        assert_eq!(Mt19937::new(1, MtMode::from_int(257)).next_int(), 895547922);
        assert!(g.mt_rand_range(10, 1).is_err());
    }

    // tests 02_engine/mt19937_value.phpt, mt19937_error.phpt
    #[test]
    fn engine() {
        let mut e = Mt19937::new(1234, MtMode::Mt19937);
        for _ in 0..10_000 {
            e.next_u32();
        }
        assert_eq!(e.generate().unwrap().bytes(), [0x60, 0xfe, 0x95, 0xd9]);
        assert_eq!(
            Mt19937::construct(Some(1234), 2).unwrap_err(),
            RandomError::value(
                "Random\\Engine\\Mt19937::__construct(): Argument #2 ($mode) must be either MT_RAND_MT19937 or MT_RAND_PHP"
            )
        );
    }

    // test ext/random/tests/02_engine/mt19937_serialize.phpt (start and end)
    #[test]
    fn serialize() {
        let st = Mt19937::new(1234, MtMode::Mt19937).state();
        assert_eq!(st.len(), 626);
        assert_eq!(st[0], StateItem::Hex("5aa6b986".into()));
        assert_eq!(st[1], StateItem::Hex("8660cc14".into()));
        assert_eq!(st[624], StateItem::Int(0));
        assert_eq!(st[625], StateItem::Int(0));
    }
}
