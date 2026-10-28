//! The random state behind `gmp_random_seed()`, `gmp_random_bits()` and
//! `gmp_random_range()`.
//!
//! PHP uses one GMP Mersenne Twister state per process, seeded with
//! `gmp_random_seed()` (or randomly on first use). Seeded streams match
//! PHP 8.5 with GMP 6.3 (test ext/gmp/tests/gmp_random_seed.phpt): the
//! seed's magnitude feeds GMP's `gmp_randseed` expansion
//! ([`MersenneTwister::gmp_seeded`]), `gmp_random_bits($n)` draws like
//! `mpz_urandomb`, and `gmp_random_range($min, $max)` is `$min` plus an
//! `mpz_urandomm` draw below `$max - $min + 1`.

use std::cell::RefCell;

use puremp::{Int, MersenneTwister};

thread_local! {
    static STATE: RefCell<Option<MersenneTwister>> = const { RefCell::new(None) };
}

fn with_state<R>(f: impl FnOnce(&mut MersenneTwister) -> R) -> R {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let mt = s.get_or_insert_with(|| {
            let mut buf = [0u8; 16];
            if goro_ext_random::secure::fill(&mut buf).is_err() {
                // No OS entropy: fall back to the hasher's random keys.
                use std::hash::{BuildHasher, Hasher};
                let mut h = std::collections::hash_map::RandomState::new().build_hasher();
                h.write_u8(0);
                buf[..8].copy_from_slice(&h.finish().to_le_bytes());
            }
            MersenneTwister::gmp_seeded(&Int::from_bytes_le(&buf))
        });
        f(mt)
    })
}

/// Seeds the state (`gmp_random_seed()`); the sign of the seed is ignored
/// (observed on PHP 8.5: seeds -100 and 100 give the same stream).
pub fn seed(seed: &Int) {
    let mt = MersenneTwister::gmp_seeded(&seed.abs());
    STATE.with(|s| *s.borrow_mut() = Some(mt));
}

/// A uniform value in `[0, 2^bits)`.
pub fn bits(bits: u64) -> Int {
    with_state(|mt| Int::random_bits(bits, mt))
}

/// A uniform value in `[min, max]` (`min < max`).
pub fn range(min: &Int, max: &Int) -> Int {
    let span = max.sub(min).add(&Int::ONE);
    let r = with_state(|mt| Int::random_below_gmp(&span, mt)).unwrap_or(Int::ZERO);
    r.add(min)
}
