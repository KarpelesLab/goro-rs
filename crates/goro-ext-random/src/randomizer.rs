//! `Random\Randomizer`'s methods, and the range reduction shared with the
//! legacy functions.
//!
//! Every function takes the engine; the Randomizer object itself only
//! holds it (its readonly `$engine` property). Errors carry PHP's full
//! messages. Behaviour (including how many `generate()` calls each method
//! makes, so that seeded sequences match) was established from the manual,
//! the RFCs, the ext/random tests and PHP 8.5 observations
//! (tests/fixtures/random.txt).

use crate::engine::Engine;
use crate::float::{IntervalBoundary, get_float as float_get};
use crate::{MSG_RETRY_LIMIT, RandomError};

/// How many rejected draws are retried before giving up (test
/// 03_randomizer/engine_unsafe_biased.phpt; observed on PHP 8.5: the 51st
/// consecutive rejection fails).
pub const RETRY_LIMIT: u32 = 50;

#[cold]
fn retry_error() -> RandomError {
    RandomError::BrokenEngine(MSG_RETRY_LIMIT)
}

/// At least 32 bits: one result, or several combined little-endian (the
/// first draw lowest) while fewer than 4 bytes were produced.
#[inline(always)]
pub fn draw32<E: Engine + ?Sized>(e: &mut E) -> Result<u32, RandomError> {
    let g = e.generate()?;
    let mut r = g.value;
    let mut total = g.size;
    while total < 4 {
        let g = e.generate()?;
        r |= g.value << (total * 8);
        total += g.size;
    }
    Ok(r as u32)
}

/// At least 64 bits, combined like [`draw32`] (bits past 64 are dropped).
#[inline(always)]
pub fn draw64<E: Engine + ?Sized>(e: &mut E) -> Result<u64, RandomError> {
    let g = e.generate()?;
    let mut r = g.value;
    let mut total = g.size;
    while total < 8 {
        let g = e.generate()?;
        r |= g.value << (total * 8);
        total += g.size;
    }
    Ok(r)
}

/// A uniform value in `0..=umax` from 32-bit draws: the draw itself for
/// the full range, a mask for powers of two, otherwise the remainder of a
/// draw below the rejection limit.
#[inline]
pub fn range32<E: Engine + ?Sized>(e: &mut E, umax: u32) -> Result<u32, RandomError> {
    let mut r = draw32(e)?;
    if umax == u32::MAX {
        return Ok(r);
    }
    let n = umax.wrapping_add(1);
    if n & umax == 0 {
        return Ok(r & umax);
    }
    // Draws above MAX - (MAX % n) - 1 are rejected; that bound is at least
    // MAX - n, so the remainder is only needed for the rare draws above it.
    if r <= u32::MAX - n {
        return Ok(r % n);
    }
    let limit = u32::MAX - (u32::MAX % n) - 1;
    let mut tries = 0;
    while r > limit {
        tries += 1;
        if tries > RETRY_LIMIT {
            return Err(retry_error());
        }
        r = draw32(e)?;
    }
    Ok(r % n)
}

/// [`range32`] with 64-bit draws.
#[inline]
pub fn range64<E: Engine + ?Sized>(e: &mut E, umax: u64) -> Result<u64, RandomError> {
    let mut r = draw64(e)?;
    if umax == u64::MAX {
        return Ok(r);
    }
    let n = umax.wrapping_add(1);
    if n & umax == 0 {
        return Ok(r & umax);
    }
    // Draws above MAX - (MAX % n) - 1 are rejected; that bound is at least
    // MAX - n, so the remainder is only needed for the rare draws above it.
    if r <= u64::MAX - n {
        return Ok(r % n);
    }
    let limit = u64::MAX - (u64::MAX % n) - 1;
    let mut tries = 0;
    while r > limit {
        tries += 1;
        if tries > RETRY_LIMIT {
            return Err(retry_error());
        }
        r = draw64(e)?;
    }
    Ok(r % n)
}

/// A uniform value in `0..=umax`: 32-bit draws when `umax` fits in 32
/// bits, 64-bit draws otherwise.
#[inline]
pub fn range_u64<E: Engine + ?Sized>(e: &mut E, umax: u64) -> Result<u64, RandomError> {
    if umax > u32::MAX as u64 {
        range64(e, umax)
    } else {
        range32(e, umax as u32).map(|v| v as u64)
    }
}

/// A uniform value in `min..=max` (`min <= max`) with the standard
/// reduction (no `MT_RAND_PHP` scaling).
#[inline]
pub fn range<E: Engine + ?Sized>(e: &mut E, min: i64, max: i64) -> Result<i64, RandomError> {
    let umax = (max as u64).wrapping_sub(min as u64);
    Ok((min as u64).wrapping_add(range_u64(e, umax)?) as i64)
}

/// [`range`] for engines that cannot fail.
#[inline]
pub(crate) fn range_unchecked<E: Engine + ?Sized>(e: &mut E, min: i64, max: i64) -> i64 {
    // Native MT draws are infallible and a rejection run of 51 has
    // probability below 2^-51; keep the first acceptable reduction.
    match range(e, min, max) {
        Ok(v) => v,
        Err(_) => min,
    }
}

/// `Randomizer::getInt($min, $max)`. A native `Mt19937` in `MT_RAND_PHP`
/// mode scales like `mt_rand()` (test 03_randomizer/compatibility_mt_rand.phpt).
pub fn get_int<E: Engine + ?Sized>(e: &mut E, min: i64, max: i64) -> Result<i64, RandomError> {
    if max < min {
        return Err(RandomError::value(
            "Random\\Randomizer::getInt(): Argument #2 ($max) must be greater than or equal to argument #1 ($min)",
        ));
    }
    if e.mt_php_scaling()
        && let Some(mt) = e.as_mt19937()
    {
        return Ok(mt.php_scaled_range(min, max));
    }
    range(e, min, max)
}

/// `Randomizer::nextInt()`: one result shifted right by one (for Mt19937
/// the same as `mt_rand()`).
#[inline]
pub fn next_int<E: Engine + ?Sized>(e: &mut E) -> Result<i64, RandomError> {
    Ok((e.generate()?.value >> 1) as i64)
}

/// `Randomizer::nextFloat()`: the top 53 of 64 bits over 2^53, in `[0, 1)`.
#[inline]
pub fn next_float<E: Engine + ?Sized>(e: &mut E) -> Result<f64, RandomError> {
    Ok((draw64(e)? >> 11) as f64 * (1.0 / (1u64 << 53) as f64))
}

/// `Randomizer::getFloat($min, $max, $boundary)` (see [`crate::float`]).
pub fn get_float<E: Engine + ?Sized>(
    e: &mut E,
    min: f64,
    max: f64,
    boundary: IntervalBoundary,
) -> Result<f64, RandomError> {
    float_get(e, min, max, boundary)
}

/// `Randomizer::getBytes($length)`: the results' bytes concatenated (each
/// at most 8 bytes) and cut to `length`.
pub fn get_bytes<E: Engine + ?Sized>(e: &mut E, length: i64) -> Result<Vec<u8>, RandomError> {
    if length < 1 {
        return Err(RandomError::value(
            "Random\\Randomizer::getBytes(): Argument #1 ($length) must be greater than 0",
        ));
    }
    let length = length as usize;
    let mut out = vec![0u8; length];
    if let Some(r) = e.fill_unobservable(&mut out) {
        r?;
        return Ok(out);
    }
    if let Some(r) = e.fill_bytes(&mut out) {
        r?;
        return Ok(out);
    }
    let mut pos = 0;
    while pos + 8 <= length {
        let g = e.generate()?;
        out[pos..pos + 8].copy_from_slice(&g.value.to_le_bytes());
        pos += g.size as usize;
    }
    while pos < length {
        let g = e.generate()?;
        let n = (g.size as usize).min(length - pos);
        out[pos..pos + n].copy_from_slice(&g.value.to_le_bytes()[..n]);
        pos += n;
    }
    Ok(out)
}

/// `Randomizer::getBytesFromString($string, $length)`.
///
/// Up to 256 source bytes, each output byte takes the next byte of the
/// current result masked to the smallest all-ones value covering the last
/// offset, rejecting offsets past the end (a new result is generated when
/// the current one is used up); more than 256 source bytes, each output
/// byte is a [`range`] draw. Test
/// 03_randomizer/methods/getBytesFromString_fast_path.phpt; observed on
/// PHP 8.5.
pub fn get_bytes_from_string<E: Engine + ?Sized>(
    e: &mut E,
    source: &[u8],
    length: i64,
) -> Result<Vec<u8>, RandomError> {
    if source.is_empty() {
        return Err(RandomError::value(
            "Random\\Randomizer::getBytesFromString(): Argument #1 ($string) must not be empty",
        ));
    }
    if length < 1 {
        return Err(RandomError::value(
            "Random\\Randomizer::getBytesFromString(): Argument #2 ($length) must be greater than 0",
        ));
    }
    let length = length as usize;
    let mut out = Vec::with_capacity(length);
    let max_offset = source.len() - 1;
    if source.len() > 256 {
        for _ in 0..length {
            let i = range_u64(e, max_offset as u64)? as usize;
            out.push(source[i]);
        }
        return Ok(out);
    }
    let mask = if max_offset == 0 {
        0
    } else {
        u64::MAX >> (max_offset as u64).leading_zeros()
    };
    let mut value = 0u64;
    let mut left = 0u32;
    for _ in 0..length {
        let mut tries = 0;
        loop {
            if left == 0 {
                let g = e.generate()?;
                value = g.value;
                left = g.size;
            }
            let off = (value & mask & 0xff) as usize;
            value >>= 8;
            left -= 1;
            if off <= max_offset {
                out.push(source[off]);
                break;
            }
            tries += 1;
            if tries > RETRY_LIMIT {
                return Err(retry_error());
            }
        }
    }
    Ok(out)
}

/// Fisher–Yates from the end: for each position from the last down to 1,
/// swap it with a [`range`] draw in `0..=position`. This is
/// `Randomizer::shuffleArray()` (on the values, renumbered),
/// `Randomizer::shuffleBytes()`, `shuffle()` and `str_shuffle()` (test
/// 03_randomizer/engine_unsafe_nul.phpt; observed on PHP 8.5).
pub fn shuffle<E: Engine + ?Sized, T>(e: &mut E, items: &mut [T]) -> Result<(), RandomError> {
    if items.len() <= 1 {
        return Ok(());
    }
    for i in (1..items.len()).rev() {
        let j = range_u64(e, i as u64)? as usize;
        items.swap(i, j);
    }
    Ok(())
}

/// `Randomizer::shuffleBytes($bytes)`, in place.
pub fn shuffle_bytes<E: Engine + ?Sized>(e: &mut E, bytes: &mut [u8]) -> Result<(), RandomError> {
    shuffle(e, bytes)
}

/// `Randomizer::shuffleArray($array)` on the array's values, in place; the
/// result is a list.
pub fn shuffle_array<E: Engine + ?Sized, T>(
    e: &mut E,
    values: &mut [T],
) -> Result<(), RandomError> {
    shuffle(e, values)
}

/// The function name for argument errors of [`pick_keys`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickCaller {
    /// `Random\Randomizer::pickArrayKeys()`.
    PickArrayKeys,
    /// `array_rand()`.
    ArrayRand,
}

impl PickCaller {
    fn prefix(self) -> &'static str {
        match self {
            PickCaller::PickArrayKeys => "Random\\Randomizer::pickArrayKeys()",
            PickCaller::ArrayRand => "array_rand()",
        }
    }
}

/// Validates `pickArrayKeys()`/`array_rand()` arguments (tests
/// 03_randomizer/methods/pickArrayKeys_error.phpt; observed on PHP 8.5 for
/// `array_rand()`).
pub fn check_pick_args(caller: PickCaller, count: usize, num: i64) -> Result<(), RandomError> {
    if count == 0 {
        return Err(RandomError::value(format!(
            "{}: Argument #1 ($array) must not be empty",
            caller.prefix()
        )));
    }
    if num < 1 || num as u64 > count as u64 {
        return Err(RandomError::value(format!(
            "{}: Argument #2 ($num) must be between 1 and the number of elements in argument #1 ($array)",
            caller.prefix()
        )));
    }
    Ok(())
}

/// The array layout [`pick_keys`] needs for a single pick.
///
/// PHP picks one element by drawing a slot of the array's storage and
/// retrying on deleted slots, unless more than half of the slots are
/// deleted, in which case it draws the element's ordinal position (observed
/// on PHP 8.5: `array_rand()` after `unset()`, tests/fixtures/random.txt).
pub trait PickSource {
    /// The number of elements.
    fn count(&self) -> usize;
    /// The number of storage slots: elements plus deleted slots before the
    /// last element (deleting the last element frees its slot).
    fn slots(&self) -> usize {
        self.count()
    }
    /// Whether slot `i` holds an element.
    fn slot_live(&self, _i: usize) -> bool {
        true
    }
    /// The ordinal position of the element in slot `i`.
    fn slot_to_index(&self, i: usize) -> usize {
        i
    }
}

/// A list or any array without deleted slots.
impl PickSource for usize {
    fn count(&self) -> usize {
        *self
    }
}

/// `Randomizer::pickArrayKeys($array, $num)` and `array_rand($array,
/// $num)`: the ordinal positions of the picked elements, in array order.
///
/// One element: see [`PickSource`]. Several: `num` distinct positions are
/// drawn with [`range`] over `0..count` into a bit set (a repeated position
/// is retried, at most [`RETRY_LIMIT`] times in a row), or, when more than
/// half the elements are picked, the `count - num` positions to leave out
/// (observed on PHP 8.5; test 03_randomizer/compatibility_array_rand.phpt).
pub fn pick_keys<E: Engine + ?Sized, S: PickSource + ?Sized>(
    e: &mut E,
    caller: PickCaller,
    src: &S,
    num: i64,
) -> Result<Vec<usize>, RandomError> {
    let count = src.count();
    check_pick_args(caller, count, num)?;
    let num = num as usize;
    if num == 1 {
        let slots = src.slots();
        if slots - count > count {
            return Ok(vec![range_u64(e, count as u64 - 1)? as usize]);
        }
        let mut tries = 0;
        loop {
            let i = range_u64(e, slots as u64 - 1)? as usize;
            if src.slot_live(i) {
                return Ok(vec![src.slot_to_index(i)]);
            }
            tries += 1;
            if tries > RETRY_LIMIT {
                return Err(retry_error());
            }
        }
    }
    let negative = num > count / 2;
    let draws = if negative { count - num } else { num };
    let mut bits = vec![0u64; count.div_ceil(64)];
    for _ in 0..draws {
        let mut tries = 0;
        loop {
            let i = range_u64(e, count as u64 - 1)? as usize;
            let (w, b) = (i / 64, 1u64 << (i % 64));
            if bits[w] & b == 0 {
                bits[w] |= b;
                break;
            }
            tries += 1;
            if tries > RETRY_LIMIT {
                return Err(retry_error());
            }
        }
    }
    let mut out = Vec::with_capacity(num);
    for i in 0..count {
        if (bits[i / 64] >> (i % 64) & 1 != 0) != negative {
            out.push(i);
        }
    }
    Ok(out)
}

/// [`pick_keys`] for `Randomizer::pickArrayKeys()` on an array of `count`
/// elements without deleted slots.
pub fn pick_array_keys<E: Engine + ?Sized>(
    e: &mut E,
    count: usize,
    num: i64,
) -> Result<Vec<usize>, RandomError> {
    pick_keys(e, PickCaller::PickArrayKeys, &count, num)
}
