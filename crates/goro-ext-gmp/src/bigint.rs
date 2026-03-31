/// Arbitrary precision integer implementation using base-2^32 limbs.
/// No external dependencies.

use std::cmp::Ordering;

const BASE: u64 = 1u64 << 32;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Sign {
    Positive,
    Negative,
    Zero,
}

#[derive(Clone, Debug)]
pub struct BigInt {
    sign: Sign,
    /// Little-endian base-2^32 limbs (least significant first)
    digits: Vec<u32>,
}

impl PartialEq for BigInt {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for BigInt {}

impl PartialOrd for BigInt {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BigInt {
    fn cmp(&self, other: &Self) -> Ordering {
        BigInt::cmp_bigint(self, other)
    }
}

impl BigInt {
    /// Create zero
    pub fn zero() -> Self {
        BigInt {
            sign: Sign::Zero,
            digits: vec![],
        }
    }

    /// Create from i64
    pub fn from_i64(n: i64) -> Self {
        if n == 0 {
            return Self::zero();
        }
        let sign = if n > 0 { Sign::Positive } else { Sign::Negative };
        let abs = (n as i128).unsigned_abs() as u64;
        let lo = abs as u32;
        let hi = (abs >> 32) as u32;
        let digits = if hi == 0 { vec![lo] } else { vec![lo, hi] };
        BigInt { sign, digits }
    }

    /// Create from string in given base (2..=62).
    /// Supports optional leading '-' or '+'.
    /// For base <= 36, digits are case-insensitive.
    /// For base > 36, lowercase a-z = 10-35, uppercase A-Z = 36-61.
    pub fn from_str(s: &str, base: u32) -> Result<Self, String> {
        if base < 2 || base > 62 {
            return Err(format!("Invalid base: {}", base));
        }
        let s = s.trim();
        if s.is_empty() {
            return Ok(Self::zero());
        }

        let bytes = s.as_bytes();
        let (negative, start) = match bytes[0] {
            b'-' => (true, 1),
            b'+' => (false, 1),
            _ => (false, 0),
        };

        if start >= bytes.len() {
            return Ok(Self::zero());
        }

        // Handle "0x", "0b", "0o" prefixes when base is 0 or 16/2/8
        let (actual_base, digit_start) = if base == 0 || base == 16 || base == 2 || base == 8 {
            if bytes.len() > start + 1 && bytes[start] == b'0' {
                match bytes[start + 1] {
                    b'x' | b'X' if base == 0 || base == 16 => (16u32, start + 2),
                    b'b' | b'B' if base == 0 || base == 2 => (2u32, start + 2),
                    b'o' | b'O' if base == 0 || base == 8 => (8u32, start + 2),
                    _ if base == 0 => (10u32, start),
                    _ => (base, start),
                }
            } else if base == 0 {
                (10u32, start)
            } else {
                (base, start)
            }
        } else {
            (base, start)
        };

        // Parse digit by digit, accumulating in chunks for efficiency
        let mut result = Self::zero();
        let chunk_size = chunk_size_for_base(actual_base);
        let chunk_multiplier = (actual_base as u64).pow(chunk_size as u32);

        let digit_bytes = &bytes[digit_start..];
        if digit_bytes.is_empty() {
            return Ok(Self::zero());
        }

        let mut i = 0;
        while i < digit_bytes.len() {
            let remaining = digit_bytes.len() - i;
            let this_chunk = if remaining < chunk_size {
                remaining
            } else {
                chunk_size
            };

            let mut chunk_val: u64 = 0;
            let actual_mult = if this_chunk < chunk_size {
                (actual_base as u64).pow(this_chunk as u32)
            } else {
                chunk_multiplier
            };

            for j in 0..this_chunk {
                let d = digit_value(digit_bytes[i + j], actual_base)?;
                chunk_val = chunk_val * (actual_base as u64) + d as u64;
            }

            result = result.mul_u64(actual_mult);
            result = result.add_u64(chunk_val);
            i += this_chunk;
        }

        if result.is_zero() {
            return Ok(Self::zero());
        }

        if negative {
            result.sign = Sign::Negative;
        }

        Ok(result)
    }

    /// Convert to string in given base (2..=62)
    pub fn to_string_radix(&self, base: u32) -> String {
        if self.is_zero() {
            return "0".to_string();
        }

        let base = if base < 2 { 10 } else if base > 62 { 10 } else { base };

        let mut result_digits = Vec::new();
        let mut temp = self.abs();

        while !temp.is_zero() {
            let (quotient, remainder) = temp.div_rem_u32(base);
            result_digits.push(digit_char(remainder, base));
            temp = quotient;
        }

        result_digits.reverse();

        let mut s = String::new();
        if self.sign == Sign::Negative {
            s.push('-');
        }
        for c in result_digits {
            s.push(c);
        }
        s
    }

    /// Convert to i64, truncating if too large
    pub fn to_i64(&self) -> i64 {
        if self.is_zero() {
            return 0;
        }
        let lo = *self.digits.first().unwrap_or(&0) as u64;
        let hi = *self.digits.get(1).unwrap_or(&0) as u64;
        let val = lo | (hi << 32);
        match self.sign {
            Sign::Positive | Sign::Zero => val as i64,
            Sign::Negative => -(val as i64),
        }
    }

    /// Check if zero
    pub fn is_zero(&self) -> bool {
        self.sign == Sign::Zero || self.digits.is_empty()
    }

    /// Return sign as -1, 0, or 1
    pub fn signum(&self) -> i32 {
        match self.sign {
            Sign::Positive => 1,
            Sign::Zero => 0,
            Sign::Negative => -1,
        }
    }

    /// Negate
    pub fn neg(&self) -> Self {
        let mut result = self.clone();
        result.sign = match result.sign {
            Sign::Positive => Sign::Negative,
            Sign::Negative => Sign::Positive,
            Sign::Zero => Sign::Zero,
        };
        result
    }

    /// Absolute value
    pub fn abs(&self) -> Self {
        let mut result = self.clone();
        if result.sign == Sign::Negative {
            result.sign = Sign::Positive;
        }
        result
    }

    /// Compare two BigInts
    pub fn cmp_bigint(a: &BigInt, b: &BigInt) -> Ordering {
        // Handle signs
        match (a.sign, b.sign) {
            (Sign::Zero, Sign::Zero) => Ordering::Equal,
            (Sign::Positive, Sign::Zero) | (Sign::Positive, Sign::Negative) => Ordering::Greater,
            (Sign::Zero, Sign::Positive) | (Sign::Negative, Sign::Positive) => Ordering::Less,
            (Sign::Negative, Sign::Zero) => Ordering::Less,
            (Sign::Zero, Sign::Negative) => Ordering::Greater,
            (Sign::Positive, Sign::Positive) => cmp_magnitude(&a.digits, &b.digits),
            (Sign::Negative, Sign::Negative) => cmp_magnitude(&b.digits, &a.digits),
        }
    }

    /// Addition
    pub fn add(&self, other: &BigInt) -> BigInt {
        match (self.sign, other.sign) {
            (Sign::Zero, _) => other.clone(),
            (_, Sign::Zero) => self.clone(),
            (Sign::Positive, Sign::Positive) => {
                let digits = add_magnitude(&self.digits, &other.digits);
                BigInt {
                    sign: Sign::Positive,
                    digits,
                }
            }
            (Sign::Negative, Sign::Negative) => {
                let digits = add_magnitude(&self.digits, &other.digits);
                BigInt {
                    sign: Sign::Negative,
                    digits,
                }
            }
            (Sign::Positive, Sign::Negative) => {
                // a + (-b) = a - b
                match cmp_magnitude(&self.digits, &other.digits) {
                    Ordering::Equal => BigInt::zero(),
                    Ordering::Greater => BigInt {
                        sign: Sign::Positive,
                        digits: sub_magnitude(&self.digits, &other.digits),
                    },
                    Ordering::Less => BigInt {
                        sign: Sign::Negative,
                        digits: sub_magnitude(&other.digits, &self.digits),
                    },
                }
            }
            (Sign::Negative, Sign::Positive) => {
                // (-a) + b = b - a
                match cmp_magnitude(&self.digits, &other.digits) {
                    Ordering::Equal => BigInt::zero(),
                    Ordering::Greater => BigInt {
                        sign: Sign::Negative,
                        digits: sub_magnitude(&self.digits, &other.digits),
                    },
                    Ordering::Less => BigInt {
                        sign: Sign::Positive,
                        digits: sub_magnitude(&other.digits, &self.digits),
                    },
                }
            }
        }
    }

    /// Subtraction
    pub fn sub(&self, other: &BigInt) -> BigInt {
        self.add(&other.neg())
    }

    /// Multiplication (schoolbook O(n*m))
    pub fn mul(&self, other: &BigInt) -> BigInt {
        if self.is_zero() || other.is_zero() {
            return BigInt::zero();
        }

        let sign = if self.sign == other.sign {
            Sign::Positive
        } else {
            Sign::Negative
        };

        let digits = mul_magnitude(&self.digits, &other.digits);
        let mut result = BigInt { sign, digits };
        result.normalize();
        result
    }

    /// Division with remainder: returns (quotient, remainder)
    /// Uses Knuth Algorithm D for multi-digit division
    pub fn div_rem(&self, other: &BigInt) -> Result<(BigInt, BigInt), String> {
        if other.is_zero() {
            return Err("Division by zero".to_string());
        }
        if self.is_zero() {
            return Ok((BigInt::zero(), BigInt::zero()));
        }

        let ord = cmp_magnitude(&self.digits, &other.digits);
        if ord == Ordering::Less {
            // |self| < |other|, quotient=0, remainder=self
            return Ok((BigInt::zero(), self.clone()));
        }
        if ord == Ordering::Equal {
            // |self| == |other|
            let q_sign = if self.sign == other.sign {
                Sign::Positive
            } else {
                Sign::Negative
            };
            return Ok((
                BigInt {
                    sign: q_sign,
                    digits: vec![1],
                },
                BigInt::zero(),
            ));
        }

        // Single-limb divisor: fast path
        if other.digits.len() == 1 {
            let d = other.digits[0];
            let (q_digits, rem) = div_rem_single(&self.digits, d);
            let q_sign = if self.sign == other.sign {
                Sign::Positive
            } else {
                Sign::Negative
            };
            let r_sign = if rem == 0 { Sign::Zero } else { self.sign };
            let mut q = BigInt {
                sign: q_sign,
                digits: q_digits,
            };
            let mut r = BigInt {
                sign: r_sign,
                digits: if rem == 0 { vec![] } else { vec![rem] },
            };
            q.normalize();
            r.normalize();
            return Ok((q, r));
        }

        // Multi-limb division: Knuth Algorithm D
        let (q_digits, r_digits) = knuth_div(&self.digits, &other.digits);

        let q_sign = if self.sign == other.sign {
            Sign::Positive
        } else {
            Sign::Negative
        };

        let mut q = BigInt {
            sign: q_sign,
            digits: q_digits,
        };
        let mut r = BigInt {
            sign: self.sign,
            digits: r_digits,
        };
        q.normalize();
        r.normalize();
        Ok((q, r))
    }

    /// Exponentiation: self^exp
    pub fn pow(&self, exp: u32) -> BigInt {
        if exp == 0 {
            return BigInt::from_i64(1);
        }
        if self.is_zero() {
            return BigInt::zero();
        }
        if exp == 1 {
            return self.clone();
        }

        // Square-and-multiply
        let mut result = BigInt::from_i64(1);
        let mut base = self.clone();
        let mut e = exp;
        while e > 0 {
            if e & 1 == 1 {
                result = result.mul(&base);
            }
            e >>= 1;
            if e > 0 {
                base = base.mul(&base);
            }
        }
        result
    }

    /// Modular exponentiation: self^exp mod modulus
    pub fn powmod(&self, exp: &BigInt, modulus: &BigInt) -> Result<BigInt, String> {
        if modulus.is_zero() {
            return Err("Division by zero".to_string());
        }

        let mod_abs = modulus.abs();

        // Handle negative exponent
        if exp.sign == Sign::Negative {
            // Need modular inverse first
            let inv = self.mod_inverse(&mod_abs)?;
            let pos_exp = exp.abs();
            return inv.powmod(&pos_exp, modulus);
        }

        if exp.is_zero() {
            let one = BigInt::from_i64(1);
            let (_, r) = one.div_rem(&mod_abs)?;
            return Ok(r);
        }

        // Square-and-multiply with modular reduction
        let mut result = BigInt::from_i64(1);
        let mut base = {
            let (_, r) = self.abs().div_rem(&mod_abs)?;
            r
        };

        // Iterate bits of exp from LSB to MSB
        let total_bits = exp.bit_length();
        for i in 0..total_bits {
            if exp.test_bit(i) {
                result = result.mul(&base);
                let (_, r) = result.div_rem(&mod_abs)?;
                result = r;
            }
            base = base.mul(&base);
            let (_, r) = base.div_rem(&mod_abs)?;
            base = r;
        }

        // Handle sign: if self is negative and exp is odd
        if self.sign == Sign::Negative && exp.test_bit(0) && !result.is_zero() {
            result = mod_abs.sub(&result);
        }

        Ok(result)
    }

    /// Greatest common divisor (Euclidean algorithm)
    pub fn gcd(&self, other: &BigInt) -> BigInt {
        let mut a = self.abs();
        let mut b = other.abs();

        while !b.is_zero() {
            let (_, r) = a.div_rem(&b).unwrap_or((BigInt::zero(), BigInt::zero()));
            a = b;
            b = r;
        }
        a
    }

    /// Integer square root (Newton's method)
    pub fn sqrt(&self) -> Result<BigInt, String> {
        if self.sign == Sign::Negative {
            return Err("Square root of negative number".to_string());
        }
        if self.is_zero() {
            return Ok(BigInt::zero());
        }

        let one = BigInt::from_i64(1);
        if *self == one {
            return Ok(one);
        }

        // Initial guess: 2^(bit_length/2)
        let bit_len = self.bit_length();
        let mut x = BigInt::from_i64(1);
        x = x.shl(bit_len / 2);

        loop {
            // x_new = (x + self/x) / 2
            let (div, _) = self.div_rem(&x)?;
            let sum = x.add(&div);
            let (x_new, _) = sum.div_rem(&BigInt::from_i64(2))?;

            if x_new >= x {
                break;
            }
            x = x_new;
        }

        Ok(x)
    }

    /// Bitwise AND (on absolute values, two's complement semantics for negative)
    pub fn bitand(&self, other: &BigInt) -> BigInt {
        bitwise_op(self, other, |a, b| a & b)
    }

    /// Bitwise OR
    pub fn bitor(&self, other: &BigInt) -> BigInt {
        bitwise_op(self, other, |a, b| a | b)
    }

    /// Bitwise XOR
    pub fn bitxor(&self, other: &BigInt) -> BigInt {
        bitwise_op(self, other, |a, b| a ^ b)
    }

    /// Bitwise NOT (one's complement, i.e., ~n = -(n+1))
    pub fn bitnot(&self) -> BigInt {
        // ~n = -(n+1) for all integers
        let one = BigInt::from_i64(1);
        self.add(&one).neg()
    }

    /// Factorial: n!
    pub fn factorial(n: u32) -> BigInt {
        if n <= 1 {
            return BigInt::from_i64(1);
        }
        let mut result = BigInt::from_i64(1);
        for i in 2..=n {
            result = result.mul(&BigInt::from_i64(i as i64));
        }
        result
    }

    /// Miller-Rabin primality test.
    /// Returns 0 = not prime, 1 = probably prime, 2 = definitely prime (for small values).
    pub fn is_probably_prime(&self, reps: u32) -> u32 {
        let n = self.abs();
        if n.is_zero() {
            return 0;
        }

        let one = BigInt::from_i64(1);
        let two = BigInt::from_i64(2);
        let three = BigInt::from_i64(3);

        if n == one {
            return 0;
        }
        if n == two || n == three {
            return 2;
        }

        // Check if even
        if !n.test_bit(0) {
            return 0;
        }

        // Check small primes for divisibility
        let small_primes: &[u32] = &[
            3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61,
            67, 71, 73, 79, 83, 89, 97, 101, 103, 107, 109, 113, 127, 131,
            137, 139, 149, 151, 157, 163, 167, 173, 179, 181, 191, 193, 197,
            199, 211, 223, 227, 229, 233, 239, 241, 251,
        ];

        for &p in small_primes {
            let pb = BigInt::from_i64(p as i64);
            if n == pb {
                return 2;
            }
            let (_, rem) = n.div_rem(&pb).unwrap();
            if rem.is_zero() {
                return 0;
            }
        }

        // If n fits in a u64 and we already tested all primes up to 251,
        // and n < 251^2 = 63001, then n is definitely prime.
        if n.digits.len() == 1 && n.digits[0] < 63001 {
            return 2;
        }

        // Write n-1 = 2^r * d
        let n_minus_1 = n.sub(&one);
        let mut d = n_minus_1.clone();
        let mut r = 0u32;
        while !d.test_bit(0) {
            d = d.shr(1);
            r += 1;
        }

        // Deterministic witnesses for small numbers
        let witnesses = deterministic_witnesses(&n);

        let actual_reps = if witnesses.is_empty() {
            reps
        } else {
            witnesses.len() as u32
        };

        // Use a simple PRNG seeded from the number itself for non-deterministic witnesses
        let mut prng_state: u64 = 0;
        for (i, &limb) in n.digits.iter().enumerate() {
            prng_state ^= (limb as u64).wrapping_mul(i as u64 + 1);
        }
        prng_state = prng_state.wrapping_add(0x9E3779B97F4A7C15);

        for i in 0..actual_reps {
            let a = if !witnesses.is_empty() {
                BigInt::from_i64(witnesses[i as usize] as i64)
            } else {
                // Generate pseudo-random witness
                prng_state ^= prng_state << 13;
                prng_state ^= prng_state >> 7;
                prng_state ^= prng_state << 17;
                let val = (prng_state % (n.to_i64().unsigned_abs().saturating_sub(3) + 1) as u64) + 2;
                BigInt::from_i64(val as i64)
            };

            // Make sure a is in range [2, n-2]
            if BigInt::cmp_bigint(&a, &n_minus_1) != Ordering::Less {
                continue;
            }
            if BigInt::cmp_bigint(&a, &two) == Ordering::Less {
                continue;
            }

            let mut x = a.powmod(&d, &n).unwrap_or(BigInt::zero());

            if x == one || x == n_minus_1 {
                continue;
            }

            let mut composite = true;
            for _ in 1..r {
                x = x.powmod(&two, &n).unwrap_or(BigInt::zero());
                if x == n_minus_1 {
                    composite = false;
                    break;
                }
            }

            if composite {
                return 0;
            }
        }

        if !witnesses.is_empty() { 2 } else { 1 }
    }

    /// Next prime after self
    pub fn next_prime(&self) -> BigInt {
        let mut candidate = self.abs();
        let one = BigInt::from_i64(1);
        let two = BigInt::from_i64(2);

        if BigInt::cmp_bigint(&candidate, &two) == Ordering::Less {
            return two;
        }

        // Make candidate odd
        if !candidate.test_bit(0) {
            candidate = candidate.add(&one);
        } else {
            candidate = candidate.add(&two);
        }

        loop {
            if candidate.is_probably_prime(25) > 0 {
                return candidate;
            }
            candidate = candidate.add(&two);
        }
    }

    /// Test bit at given index (0-indexed from LSB)
    pub fn test_bit(&self, index: usize) -> bool {
        let limb_index = index / 32;
        let bit_index = index % 32;
        if self.sign == Sign::Negative {
            // Two's complement: bits of -n are bits of ~(n-1)
            let pos = self.abs();
            let one = BigInt::from_i64(1);
            let n_minus_1 = pos.sub(&one);
            let limb = *n_minus_1.digits.get(limb_index).unwrap_or(&0);
            // Invert the bit (two's complement)
            (limb >> bit_index) & 1 == 0
        } else {
            let limb = *self.digits.get(limb_index).unwrap_or(&0);
            (limb >> bit_index) & 1 == 1
        }
    }

    /// Set or clear a bit at given index
    pub fn set_bit(&self, index: usize, value: bool) -> BigInt {
        if self.sign == Sign::Negative {
            // Two's complement manipulation for negative numbers
            let pos = self.abs();
            let one = BigInt::from_i64(1);
            let n_minus_1 = pos.sub(&one);
            // In two's complement, bit of -n at position i = ~bit of (n-1) at position i
            // Setting bit i in -n means clearing bit i in (n-1), then re-negate
            let mut modified = n_minus_1;
            let limb_index = index / 32;
            let bit_index = index % 32;
            while modified.digits.len() <= limb_index {
                modified.digits.push(0);
            }
            if value {
                // Setting bit in negative number = clearing bit in (|n|-1)
                modified.digits[limb_index] &= !(1u32 << bit_index);
            } else {
                // Clearing bit in negative number = setting bit in (|n|-1)
                modified.digits[limb_index] |= 1u32 << bit_index;
            }
            modified.normalize();
            modified.add(&one).neg()
        } else {
            let mut result = self.clone();
            let limb_index = index / 32;
            let bit_index = index % 32;
            while result.digits.len() <= limb_index {
                result.digits.push(0);
            }
            if value {
                result.digits[limb_index] |= 1u32 << bit_index;
            } else {
                result.digits[limb_index] &= !(1u32 << bit_index);
            }
            result.normalize();
            result
        }
    }

    /// Population count (number of 1-bits). For negative numbers, returns u64::MAX per PHP semantics.
    pub fn popcount(&self) -> i64 {
        if self.sign == Sign::Negative {
            return -1; // PHP returns -1 for negative numbers
        }
        let mut count: i64 = 0;
        for &limb in &self.digits {
            count += limb.count_ones() as i64;
        }
        count
    }

    /// Check if perfect square
    pub fn is_perfect_square(&self) -> bool {
        if self.sign == Sign::Negative {
            return false;
        }
        if self.is_zero() {
            return true;
        }
        match self.sqrt() {
            Ok(root) => root.mul(&root) == *self,
            Err(_) => false,
        }
    }

    /// Modular inverse using extended GCD: returns a^-1 mod m
    pub fn mod_inverse(&self, modulus: &BigInt) -> Result<BigInt, String> {
        if modulus.is_zero() {
            return Err("Division by zero".to_string());
        }
        let (g, x, _) = extended_gcd(&self.abs(), &modulus.abs());
        let one = BigInt::from_i64(1);
        if g != one {
            return Err("Inverse doesn't exist".to_string());
        }
        // Adjust sign
        let result = if self.sign == Sign::Negative {
            x.neg()
        } else {
            x
        };
        // Make positive
        let m = modulus.abs();
        let (_, mut r) = result.div_rem(&m)?;
        if r.sign == Sign::Negative {
            r = r.add(&m);
        }
        Ok(r)
    }

    /// Bit length (number of bits to represent absolute value)
    pub fn bit_length(&self) -> usize {
        if self.is_zero() {
            return 0;
        }
        let top = *self.digits.last().unwrap();
        let top_bits = 32 - top.leading_zeros() as usize;
        (self.digits.len() - 1) * 32 + top_bits
    }

    // === Internal helpers ===

    /// Normalize: remove leading zero limbs, fix sign
    fn normalize(&mut self) {
        while self.digits.last() == Some(&0) {
            self.digits.pop();
        }
        if self.digits.is_empty() {
            self.sign = Sign::Zero;
        }
    }

    /// Multiply by a u64 (used in string parsing)
    fn mul_u64(&self, n: u64) -> BigInt {
        if self.is_zero() || n == 0 {
            return BigInt::zero();
        }
        if n == 1 {
            return self.clone();
        }

        let lo = n as u32;
        let hi = (n >> 32) as u32;

        if hi == 0 {
            // Single-limb multiply
            let mut result = Vec::with_capacity(self.digits.len() + 1);
            let mut carry: u64 = 0;
            for &d in &self.digits {
                let prod = d as u64 * lo as u64 + carry;
                result.push(prod as u32);
                carry = prod >> 32;
            }
            if carry > 0 {
                result.push(carry as u32);
            }
            let mut r = BigInt {
                sign: self.sign,
                digits: result,
            };
            r.normalize();
            r
        } else {
            let other = BigInt {
                sign: Sign::Positive,
                digits: vec![lo, hi],
            };
            self.mul(&other)
        }
    }

    /// Add a u64 (used in string parsing)
    fn add_u64(&self, n: u64) -> BigInt {
        if n == 0 {
            return self.clone();
        }
        let other = BigInt {
            sign: Sign::Positive,
            digits: if (n >> 32) == 0 {
                vec![n as u32]
            } else {
                vec![n as u32, (n >> 32) as u32]
            },
        };
        self.add(&other)
    }

    /// Divide by a single u32, returning (quotient_digits, remainder)
    fn div_rem_u32(&self, divisor: u32) -> (BigInt, u32) {
        let (q_digits, rem) = div_rem_single(&self.digits, divisor);
        let mut q = BigInt {
            sign: self.sign,
            digits: q_digits,
        };
        q.normalize();
        (q, rem)
    }

    /// Shift left by n bits
    fn shl(&self, n: usize) -> BigInt {
        if self.is_zero() || n == 0 {
            return self.clone();
        }
        let limb_shift = n / 32;
        let bit_shift = n % 32;

        let mut result = vec![0u32; limb_shift];

        if bit_shift == 0 {
            result.extend_from_slice(&self.digits);
        } else {
            let mut carry: u32 = 0;
            for &d in &self.digits {
                let shifted = ((d as u64) << bit_shift) | carry as u64;
                result.push(shifted as u32);
                carry = (shifted >> 32) as u32;
            }
            if carry > 0 {
                result.push(carry);
            }
        }

        let mut r = BigInt {
            sign: self.sign,
            digits: result,
        };
        r.normalize();
        r
    }

    /// Shift right by n bits
    fn shr(&self, n: usize) -> BigInt {
        if self.is_zero() || n == 0 {
            return self.clone();
        }
        let limb_shift = n / 32;
        let bit_shift = n % 32;

        if limb_shift >= self.digits.len() {
            return BigInt::zero();
        }

        let src = &self.digits[limb_shift..];
        let mut result = Vec::with_capacity(src.len());

        if bit_shift == 0 {
            result.extend_from_slice(src);
        } else {
            for i in 0..src.len() {
                let lo = src[i] >> bit_shift;
                let hi = if i + 1 < src.len() {
                    src[i + 1] << (32 - bit_shift)
                } else {
                    0
                };
                result.push(lo | hi);
            }
        }

        let mut r = BigInt {
            sign: self.sign,
            digits: result,
        };
        r.normalize();
        r
    }
}

impl std::fmt::Display for BigInt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string_radix(10))
    }
}

// === Magnitude-only operations (on Vec<u32> digits, little-endian) ===

/// Compare magnitudes
fn cmp_magnitude(a: &[u32], b: &[u32]) -> Ordering {
    let a_len = a.len();
    let b_len = b.len();
    if a_len != b_len {
        return a_len.cmp(&b_len);
    }
    // Same length: compare from most significant
    for i in (0..a_len).rev() {
        match a[i].cmp(&b[i]) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

/// Add magnitudes
fn add_magnitude(a: &[u32], b: &[u32]) -> Vec<u32> {
    let max_len = a.len().max(b.len());
    let mut result = Vec::with_capacity(max_len + 1);
    let mut carry: u64 = 0;

    for i in 0..max_len {
        let av = *a.get(i).unwrap_or(&0) as u64;
        let bv = *b.get(i).unwrap_or(&0) as u64;
        let sum = av + bv + carry;
        result.push(sum as u32);
        carry = sum >> 32;
    }
    if carry > 0 {
        result.push(carry as u32);
    }
    result
}

/// Subtract magnitudes (assumes a >= b)
fn sub_magnitude(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut result = Vec::with_capacity(a.len());
    let mut borrow: u64 = 0;

    for i in 0..a.len() {
        let av = a[i] as u64;
        let bv = *b.get(i).unwrap_or(&0) as u64;
        let diff = av.wrapping_sub(bv).wrapping_sub(borrow);
        if av < bv + borrow {
            // Need to borrow
            result.push((diff.wrapping_add(BASE)) as u32);
            borrow = 1;
        } else {
            result.push(diff as u32);
            borrow = 0;
        }
    }

    // Remove leading zeros
    while result.last() == Some(&0) {
        result.pop();
    }
    result
}

/// Schoolbook multiplication
fn mul_magnitude(a: &[u32], b: &[u32]) -> Vec<u32> {
    if a.is_empty() || b.is_empty() {
        return vec![];
    }
    let mut result = vec![0u32; a.len() + b.len()];

    for i in 0..a.len() {
        let mut carry: u64 = 0;
        for j in 0..b.len() {
            let prod = (a[i] as u64) * (b[j] as u64) + result[i + j] as u64 + carry;
            result[i + j] = prod as u32;
            carry = prod >> 32;
        }
        result[i + b.len()] += carry as u32;
    }

    // Remove leading zeros
    while result.last() == Some(&0) {
        result.pop();
    }
    result
}

/// Division by single u32 limb. Returns (quotient_digits, remainder).
fn div_rem_single(a: &[u32], d: u32) -> (Vec<u32>, u32) {
    let d = d as u64;
    let mut result = vec![0u32; a.len()];
    let mut rem: u64 = 0;

    for i in (0..a.len()).rev() {
        let cur = (rem << 32) | a[i] as u64;
        result[i] = (cur / d) as u32;
        rem = cur % d;
    }

    // Remove leading zeros
    while result.last() == Some(&0) {
        result.pop();
    }
    (result, rem as u32)
}

/// Knuth Algorithm D: multi-limb division.
/// Returns (quotient, remainder) as digit vectors.
/// Assumes divisor has at least 2 limbs and dividend >= divisor in magnitude.
fn knuth_div(u: &[u32], v: &[u32]) -> (Vec<u32>, Vec<u32>) {
    let n = v.len();
    let m = u.len() - n;

    // Step D1: Normalize. Shift so that v's leading limb has high bit set.
    let shift = v[n - 1].leading_zeros();
    let mut un = vec![0u32; u.len() + 1]; // normalized dividend (one extra limb)
    let mut vn = vec![0u32; n]; // normalized divisor

    // Shift divisor left by `shift` bits
    if shift > 0 {
        let mut carry = 0u32;
        for i in 0..n {
            let val = ((v[i] as u64) << shift) | carry as u64;
            vn[i] = val as u32;
            carry = (val >> 32) as u32;
        }

        // Shift dividend left by `shift` bits
        carry = 0;
        for i in 0..u.len() {
            let val = ((u[i] as u64) << shift) | carry as u64;
            un[i] = val as u32;
            carry = (val >> 32) as u32;
        }
        un[u.len()] = carry;
    } else {
        vn.copy_from_slice(v);
        un[..u.len()].copy_from_slice(u);
    }

    let mut q = vec![0u32; m + 1];

    // Step D2-D7: Main loop
    for j in (0..=m).rev() {
        // Step D3: Calculate q_hat
        let u_hi = (un[j + n] as u64) << 32 | un[j + n - 1] as u64;
        let mut q_hat = u_hi / vn[n - 1] as u64;
        let mut r_hat = u_hi % vn[n - 1] as u64;

        // Refine q_hat
        loop {
            if q_hat >= BASE
                || (n >= 2
                    && q_hat * vn[n - 2] as u64 > (r_hat << 32) + un[j + n - 2] as u64)
            {
                q_hat -= 1;
                r_hat += vn[n - 1] as u64;
                if r_hat < BASE {
                    continue;
                }
            }
            break;
        }

        // Step D4: Multiply and subtract
        let mut borrow: i64 = 0;
        for i in 0..n {
            let prod = q_hat * vn[i] as u64;
            let diff = un[j + i] as i64 - borrow - (prod as u32) as i64;
            un[j + i] = diff as u32;
            borrow = (prod >> 32) as i64 - (diff >> 32);
        }
        let diff = un[j + n] as i64 - borrow;
        un[j + n] = diff as u32;

        // Step D5: Test remainder
        q[j] = q_hat as u32;
        if diff < 0 {
            // Step D6: Add back
            q[j] -= 1;
            let mut carry: u64 = 0;
            for i in 0..n {
                let sum = un[j + i] as u64 + vn[i] as u64 + carry;
                un[j + i] = sum as u32;
                carry = sum >> 32;
            }
            un[j + n] = un[j + n].wrapping_add(carry as u32);
        }
    }

    // Step D8: Un-normalize remainder
    let mut rem = vec![0u32; n];
    if shift > 0 {
        for i in 0..n {
            rem[i] = (un[i] >> shift)
                | if i + 1 < n {
                    un[i + 1] << (32 - shift)
                } else {
                    0
                };
        }
    } else {
        rem[..n].copy_from_slice(&un[..n]);
    }

    // Remove leading zeros
    while q.last() == Some(&0) {
        q.pop();
    }
    while rem.last() == Some(&0) {
        rem.pop();
    }

    (q, rem)
}

/// Extended GCD: returns (gcd, x, y) such that a*x + b*y = gcd
fn extended_gcd(a: &BigInt, b: &BigInt) -> (BigInt, BigInt, BigInt) {
    if b.is_zero() {
        return (a.clone(), BigInt::from_i64(1), BigInt::zero());
    }

    let (_, rem) = a.div_rem(b).unwrap();
    let (g, x1, y1) = extended_gcd(b, &rem);

    let (quot, _) = a.div_rem(b).unwrap();
    let y = x1.sub(&quot.mul(&y1));

    (g, y1, y)
}

/// Bitwise operation on BigInts using two's complement representation
fn bitwise_op<F: Fn(u32, u32) -> u32>(a: &BigInt, b: &BigInt, op: F) -> BigInt {
    // Convert to two's complement representation
    let a_tc = to_twos_complement(a);
    let b_tc = to_twos_complement(b);

    let a_neg = a.sign == Sign::Negative;
    let b_neg = b.sign == Sign::Negative;
    let a_fill: u32 = if a_neg { 0xFFFFFFFF } else { 0 };
    let b_fill: u32 = if b_neg { 0xFFFFFFFF } else { 0 };

    let max_len = a_tc.len().max(b_tc.len());
    let mut result = Vec::with_capacity(max_len);

    for i in 0..max_len {
        let av = *a_tc.get(i).unwrap_or(&a_fill);
        let bv = *b_tc.get(i).unwrap_or(&b_fill);
        result.push(op(av, bv));
    }

    // Check sign of result: if op on the fill values produces all-1s, result is negative
    let result_neg = op(a_fill, b_fill) == 0xFFFFFFFF;

    from_twos_complement(&result, result_neg)
}

/// Convert BigInt to two's complement limbs
fn to_twos_complement(n: &BigInt) -> Vec<u32> {
    if n.is_zero() {
        return vec![0];
    }
    if n.sign != Sign::Negative {
        return n.digits.clone();
    }
    // For negative: two's complement = invert all bits of (|n|-1)
    let one = BigInt::from_i64(1);
    let abs_minus_1 = n.abs().sub(&one);
    let mut digits = if abs_minus_1.is_zero() {
        vec![0u32]
    } else {
        abs_minus_1.digits.clone()
    };
    // Invert all bits
    for d in &mut digits {
        *d = !*d;
    }
    digits
}

/// Convert two's complement limbs back to BigInt
fn from_twos_complement(digits: &[u32], negative: bool) -> BigInt {
    if !negative {
        let mut d = digits.to_vec();
        while d.last() == Some(&0) {
            d.pop();
        }
        if d.is_empty() {
            return BigInt::zero();
        }
        BigInt {
            sign: Sign::Positive,
            digits: d,
        }
    } else {
        // Invert all bits, add 1
        let mut inverted: Vec<u32> = digits.iter().map(|d| !d).collect();
        // Remove leading zeros
        while inverted.last() == Some(&0) {
            inverted.pop();
        }
        if inverted.is_empty() {
            // All bits were 1s -> inverted is 0 -> value is -(0+1) = -1
            return BigInt::from_i64(-1);
        }
        let mut result = BigInt {
            sign: Sign::Positive,
            digits: inverted,
        };
        result = result.add(&BigInt::from_i64(1));
        result.sign = Sign::Negative;
        result
    }
}

/// Get deterministic Miller-Rabin witnesses for numbers below certain thresholds
fn deterministic_witnesses(n: &BigInt) -> Vec<u64> {
    // For numbers that fit in a u64, we can use known deterministic witness sets
    if n.digits.len() <= 2 {
        let val = n.to_i64().unsigned_abs();
        if val < 2_047 {
            return vec![2];
        }
        if val < 1_373_653 {
            return vec![2, 3];
        }
        if val < 9_080_191 {
            return vec![31, 73];
        }
        if val < 25_326_001 {
            return vec![2, 3, 5];
        }
        if val < 3_215_031_751 {
            return vec![2, 3, 5, 7];
        }
        if val < 4_759_123_141 {
            return vec![2, 7, 61];
        }
    }
    vec![] // Use random witnesses
}

/// How many digits to process in a chunk for string parsing in given base
fn chunk_size_for_base(base: u32) -> usize {
    // Largest k such that base^k < 2^32
    match base {
        2 => 31,
        3 => 20,
        4 => 15,
        5 => 13,
        6 => 12,
        7 => 11,
        8 => 10,
        9 => 10,
        10 => 9,
        11 => 9,
        12 => 8,
        13 => 8,
        14 => 8,
        15 => 8,
        16 => 7,
        b if b <= 23 => 6,
        b if b <= 40 => 5,
        b if b <= 62 => 4,
        _ => 4,
    }
}

/// Convert a character to its digit value in the given base
fn digit_value(ch: u8, base: u32) -> Result<u32, String> {
    let val = match ch {
        b'0'..=b'9' => (ch - b'0') as u32,
        b'a'..=b'z' if base <= 36 => (ch - b'a' + 10) as u32,
        b'A'..=b'Z' if base <= 36 => (ch - b'A' + 10) as u32,
        // For base > 36: lowercase = 10-35, uppercase = 36-61
        b'a'..=b'z' => (ch - b'a' + 10) as u32,
        b'A'..=b'Z' => (ch - b'A' + 36) as u32,
        _ => return Err(format!("Invalid digit '{}' for base {}", ch as char, base)),
    };
    if val >= base {
        return Err(format!(
            "Digit '{}' (value {}) out of range for base {}",
            ch as char, val, base
        ));
    }
    Ok(val)
}

/// Convert a digit value to its character representation in the given base
fn digit_char(val: u32, base: u32) -> char {
    if val < 10 {
        (b'0' + val as u8) as char
    } else if base <= 36 {
        (b'a' + (val - 10) as u8) as char
    } else {
        // For base > 36: 10-35 = a-z, 36-61 = A-Z
        if val < 36 {
            (b'a' + (val - 10) as u8) as char
        } else {
            (b'A' + (val - 36) as u8) as char
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_i64() {
        assert_eq!(BigInt::from_i64(0).to_string_radix(10), "0");
        assert_eq!(BigInt::from_i64(42).to_string_radix(10), "42");
        assert_eq!(BigInt::from_i64(-42).to_string_radix(10), "-42");
        assert_eq!(
            BigInt::from_i64(i64::MAX).to_string_radix(10),
            "9223372036854775807"
        );
        assert_eq!(
            BigInt::from_i64(i64::MIN + 1).to_string_radix(10),
            "-9223372036854775807"
        );
    }

    #[test]
    fn test_from_str() {
        assert_eq!(
            BigInt::from_str("12345", 10).unwrap().to_string_radix(10),
            "12345"
        );
        assert_eq!(
            BigInt::from_str("-12345", 10).unwrap().to_string_radix(10),
            "-12345"
        );
        assert_eq!(
            BigInt::from_str("ff", 16).unwrap().to_string_radix(10),
            "255"
        );
        assert_eq!(
            BigInt::from_str("11111111", 2).unwrap().to_string_radix(10),
            "255"
        );
        assert_eq!(
            BigInt::from_str("999999999999999999999", 10)
                .unwrap()
                .to_string_radix(10),
            "999999999999999999999"
        );
    }

    #[test]
    fn test_add() {
        let a = BigInt::from_i64(100);
        let b = BigInt::from_i64(200);
        assert_eq!(a.add(&b).to_string_radix(10), "300");

        let c = BigInt::from_i64(-50);
        assert_eq!(a.add(&c).to_string_radix(10), "50");
    }

    #[test]
    fn test_mul() {
        let a = BigInt::from_i64(12345);
        let b = BigInt::from_i64(67890);
        assert_eq!(a.mul(&b).to_string_radix(10), "838102050");
    }

    #[test]
    fn test_div() {
        let a = BigInt::from_i64(100);
        let b = BigInt::from_i64(7);
        let (q, r) = a.div_rem(&b).unwrap();
        assert_eq!(q.to_string_radix(10), "14");
        assert_eq!(r.to_string_radix(10), "2");
    }

    #[test]
    fn test_pow() {
        let a = BigInt::from_i64(2);
        assert_eq!(a.pow(10).to_string_radix(10), "1024");
    }

    #[test]
    fn test_factorial() {
        assert_eq!(
            BigInt::factorial(20).to_string_radix(10),
            "2432902008176640000"
        );
    }

    #[test]
    fn test_gcd() {
        let a = BigInt::from_i64(48);
        let b = BigInt::from_i64(18);
        assert_eq!(a.gcd(&b).to_string_radix(10), "6");
    }

    #[test]
    fn test_primality() {
        assert!(BigInt::from_i64(2).is_probably_prime(10) > 0);
        assert!(BigInt::from_i64(17).is_probably_prime(10) > 0);
        assert_eq!(BigInt::from_i64(4).is_probably_prime(10), 0);
        assert_eq!(BigInt::from_i64(100).is_probably_prime(10), 0);
    }

    #[test]
    fn test_sqrt() {
        assert_eq!(
            BigInt::from_i64(144).sqrt().unwrap().to_string_radix(10),
            "12"
        );
        assert_eq!(
            BigInt::from_i64(10).sqrt().unwrap().to_string_radix(10),
            "3"
        );
    }

    #[test]
    fn test_bitwise() {
        let a = BigInt::from_i64(0xFF);
        let b = BigInt::from_i64(0x0F);
        assert_eq!(a.bitand(&b).to_i64(), 0x0F);
        assert_eq!(a.bitor(&b).to_i64(), 0xFF);
        assert_eq!(a.bitxor(&b).to_i64(), 0xF0);
    }

    #[test]
    fn test_bitnot() {
        // ~0 = -1, ~1 = -2, ~(-1) = 0
        assert_eq!(BigInt::from_i64(0).bitnot().to_i64(), -1);
        assert_eq!(BigInt::from_i64(1).bitnot().to_i64(), -2);
        assert_eq!(BigInt::from_i64(-1).bitnot().to_i64(), 0);
    }

    #[test]
    fn test_next_prime() {
        assert_eq!(BigInt::from_i64(10).next_prime().to_i64(), 11);
        assert_eq!(BigInt::from_i64(11).next_prime().to_i64(), 13);
    }

    #[test]
    fn test_powmod() {
        // 2^10 mod 1000 = 1024 mod 1000 = 24
        let base = BigInt::from_i64(2);
        let exp = BigInt::from_i64(10);
        let modulus = BigInt::from_i64(1000);
        assert_eq!(base.powmod(&exp, &modulus).unwrap().to_i64(), 24);
    }

    #[test]
    fn test_mod_inverse() {
        // 3^-1 mod 11 = 4 (because 3*4 = 12 = 1 mod 11)
        let a = BigInt::from_i64(3);
        let m = BigInt::from_i64(11);
        assert_eq!(a.mod_inverse(&m).unwrap().to_i64(), 4);
    }

    #[test]
    fn test_base_conversion() {
        let n = BigInt::from_i64(255);
        assert_eq!(n.to_string_radix(16), "ff");
        assert_eq!(n.to_string_radix(2), "11111111");
        assert_eq!(n.to_string_radix(8), "377");
    }

    #[test]
    fn test_large_multiply() {
        // Test with numbers larger than u64
        let a = BigInt::from_str("999999999999999999999999999", 10).unwrap();
        let b = BigInt::from_str("999999999999999999999999999", 10).unwrap();
        let result = a.mul(&b);
        assert_eq!(
            result.to_string_radix(10),
            "999999999999999999999999998000000000000000000000000001"
        );
    }
}
