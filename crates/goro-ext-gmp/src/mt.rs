//! GMP-compatible Mersenne Twister (`gmp_randinit_mt`).
//!
//! PHP's gmp_random_* functions sit on GMP's MT19937 state, and scripts
//! seeding it with gmp_random_seed() expect GMP's exact stream. The core
//! generator is the reference MT19937; what is GMP-specific is the seeding
//! (the seed is reduced mod 2^19937-20027, offset by 2 and raised to the
//! power 1074888996 mod 2^19937-20023, then spread over the state words and
//! warmed up by 2000 outputs) and how outputs are packed into 64-bit limbs
//! by mpz_urandomb() / mpz_urandomm().

use puremp::{Int, Nat};

const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908_B0DF;
const WARM_UP: usize = 2000;
/// mpz_urandomm() gives up rejection sampling after this many draws.
const MAX_URANDOMM_ITER: u32 = 80;

pub struct GmpMt {
    mt: [u32; N],
    mti: usize,
}

/// One MT19937 twist of the whole state.
fn recalc_buffer(mt: &mut [u32; N]) {
    let mix = |hi: u32, lo: u32, other: u32| {
        let y = (hi & 0x8000_0000) | (lo & 0x7FFF_FFFF);
        other ^ (y >> 1) ^ if y & 1 != 0 { MATRIX_A } else { 0 }
    };
    for kk in 0..N - M {
        mt[kk] = mix(mt[kk], mt[kk + 1], mt[kk + M]);
    }
    for kk in N - M..N - 1 {
        mt[kk] = mix(mt[kk], mt[kk + 1], mt[kk + M - N]);
    }
    mt[N - 1] = mix(mt[N - 1], mt[0], mt[M - 1]);
}

/// `r = r^1074888996 mod (2^19937 - 20023)`, with GMP's (lazy) reduction:
/// the value is only folded until it is below 2^19937.
fn mangle_seed(r: &Nat) -> Nat {
    fn reduce(mut r: Nat) -> Nat {
        loop {
            let t = r.shr(19937);
            if t.is_zero() {
                return r;
            }
            r = r.low_bits(19937).add(&t.mul(&Nat::from_u64(20023)));
        }
    }
    let b = r.clone();
    let mut r = r.clone();
    let mut e: u32 = 0x4011_8124;
    let mut bit: u32 = 0x2000_0000;
    while bit != 0 {
        r = reduce(r.square());
        if e & bit != 0 {
            e ^= bit;
            r = reduce(r.mul(&b));
        }
        bit >>= 1;
    }
    r
}

impl GmpMt {
    /// gmp_randinit_mt() followed by gmp_randseed(seed).
    pub fn with_seed(seed: &Int) -> GmpMt {
        let modulus = Int::from(Nat::one().shl(19937)) - Int::from(20027u64);
        let seed1 = (seed.rem_euclid(&modulus) + Int::from(2u64)).into_magnitude();
        let seed2 = mangle_seed(&seed1);

        let mut mt = [0u32; N];
        // The top bit (19936) goes to bit 31 of mt[0]; the rest is split into
        // 32-bit words, least significant first, from mt[1] on.
        mt[0] = if seed2.bit(19936) { 0x8000_0000 } else { 0 };
        let rest = seed2.low_bits(19936);
        let mut i = 1;
        for &limb in rest.as_limbs() {
            for word in [limb as u32, (limb >> 32) as u32] {
                if i < N {
                    mt[i] = word;
                    i += 1;
                }
            }
        }
        for _ in 0..WARM_UP / N {
            recalc_buffer(&mut mt);
        }
        GmpMt {
            mt,
            mti: WARM_UP % N,
        }
    }

    fn next_u32(&mut self) -> u32 {
        if self.mti >= N {
            recalc_buffer(&mut self.mt);
            self.mti = 0;
        }
        let mut y = self.mt[self.mti];
        self.mti += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9D2C_5680;
        y ^= (y << 15) & 0xEFC6_0000;
        y ^= y >> 18;
        y
    }

    /// GMP's `randget_mt` for 64-bit limbs: `nbits` random bits as limbs.
    fn get_bits(&mut self, nbits: u64) -> Vec<u64> {
        let nlimbs = (nbits / 64) as usize;
        let rbits = (nbits % 64) as u32;
        let mut out = Vec::with_capacity(nlimbs + 1);
        for _ in 0..nlimbs {
            let lo = self.next_u32() as u64;
            let hi = self.next_u32() as u64;
            out.push(lo | (hi << 32));
        }
        if rbits > 0 {
            let y = self.next_u32() as u64;
            if rbits < 32 {
                out.push(y & ((1u64 << rbits) - 1));
            } else {
                let mut limb = y;
                if rbits > 32 {
                    let y2 = self.next_u32() as u64;
                    limb |= (y2 & ((1u64 << (rbits - 32)) - 1)) << 32;
                }
                out.push(limb);
            }
        }
        out
    }

    /// mpz_urandomb(): uniform in [0, 2^nbits).
    pub fn urandomb(&mut self, nbits: u64) -> Int {
        Int::from(Nat::from_limbs(&self.get_bits(nbits)))
    }

    /// mpz_urandomm(): uniform in [0, n) for n > 0.
    pub fn urandomm(&mut self, n: &Int) -> Int {
        let n = n.abs();
        let pow2 = n.is_power_of_two().is_some();
        let nbits = n.bit_len() as u64 - pow2 as u64;
        if nbits == 0 {
            return Int::ZERO;
        }
        let mut r;
        let mut count = MAX_URANDOMM_ITER;
        loop {
            r = self.urandomb(nbits);
            count -= 1;
            if r < n || count == 0 {
                break;
            }
        }
        if r >= n { r - n } else { r }
    }
}
