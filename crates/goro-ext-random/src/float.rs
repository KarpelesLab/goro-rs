//! `Randomizer::getFloat()`: the γ-section algorithm (Goualard, "Drawing
//! random floating-point numbers from an interval", ACM TOMACS 32(3),
//! 2022), as adopted by the "Randomizer::getFloat() and nextFloat()" RFC.
//!
//! The interval is cut into steps of γ, the larger spacing of floats at its
//! two ends; an integer `k` is drawn uniformly and the result is computed
//! from the endpoint with the smaller magnitude's opposite (so the step is
//! exact), the far endpoint being returned exactly for the last `k`.
//! Checked against tests 03_randomizer/methods/getFloat_*.phpt and PHP 8.5
//! observations (tests/fixtures/random.txt).

use crate::RandomError;
use crate::engine::Engine;
use crate::randomizer::range64;

/// `Random\IntervalBoundary`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntervalBoundary {
    ClosedOpen,
    ClosedClosed,
    OpenClosed,
    OpenOpen,
}

impl IntervalBoundary {
    /// The enum cases in declaration order.
    pub const CASES: [IntervalBoundary; 4] = [
        IntervalBoundary::ClosedOpen,
        IntervalBoundary::ClosedClosed,
        IntervalBoundary::OpenClosed,
        IntervalBoundary::OpenOpen,
    ];

    /// The case name.
    pub fn name(self) -> &'static str {
        match self {
            IntervalBoundary::ClosedOpen => "ClosedOpen",
            IntervalBoundary::ClosedClosed => "ClosedClosed",
            IntervalBoundary::OpenClosed => "OpenClosed",
            IntervalBoundary::OpenOpen => "OpenOpen",
        }
    }

    /// The case with this name.
    pub fn from_name(name: &str) -> Option<IntervalBoundary> {
        Self::CASES.into_iter().find(|c| c.name() == name)
    }
}

/// γ: the larger of the spacing above `a` and below `b`.
fn gamma(a: f64, b: f64) -> f64 {
    let lo = a.next_up() - a;
    let hi = b - b.next_down();
    if lo > hi { lo } else { hi }
}

/// `x / g` for the power of two `g`: a multiplication when `1 / g` is
/// finite (then both are the same exact scaling).
#[inline(always)]
fn div_g(x: f64, g: f64, inv: f64) -> f64 {
    if inv.is_finite() { x * inv } else { x / g }
}

/// ⌈(b - a) / g⌉, computed without overflow and corrected for the
/// rounding of the quotient difference (the paper's `ceilint`).
#[inline]
fn ceilint(a: f64, b: f64, g: f64, inv: f64) -> u64 {
    let (ag, bg) = (div_g(a, g, inv), div_g(b, g, inv));
    let s = bg - ag;
    let eps = if a.abs() <= b.abs() {
        -ag - (s - bg)
    } else {
        bg - (s + ag)
    };
    let si = s.ceil();
    if s != si {
        si as u64
    } else {
        si as u64 + (eps > 0.0) as u64
    }
}

/// `x - k * g` (`down`) or `x + k * g`, as PHP 8.5 computes it.
///
/// Observed on PHP 8.5 (tests/fixtures/random.txt): the result is the exact
/// value, even where `k` exceeds 2^53 or the product overflows, except that
/// the endpoint first goes through a division and multiplication by 4, which
/// rounds a subnormal endpoint to a multiple of 4 × 2^-1074 (so on such
/// tiny intervals the results are shifted by up to two steps, and can even
/// fall outside the interval).
#[inline]
fn step(x: f64, k: u64, g: f64, inv: f64, down: bool) -> f64 {
    let x = (x * 0.25) * 4.0;
    let units = div_g(x, g, inv);
    if units.fract() != 0.0 || units.abs() > (1u64 << 62) as f64 {
        // Not a multiple of γ (never seen): plain floating point.
        let p = k as f64 * g;
        return if down { x - p } else { x + p };
    }
    // γ is a power of two, so the result is an integer number of steps
    // (below 2^62 in magnitude): compute it exactly, then round once.
    let m = units as i64;
    let k = k as i64;
    let m = if down { m - k } else { m + k };
    m as f64 * g
}

fn check(min: f64, max: f64, closed: bool) -> Result<(), RandomError> {
    if !min.is_finite() {
        return Err(RandomError::value(
            "Random\\Randomizer::getFloat(): Argument #1 ($min) must be finite",
        ));
    }
    if !max.is_finite() {
        return Err(RandomError::value(
            "Random\\Randomizer::getFloat(): Argument #2 ($max) must be finite",
        ));
    }
    if closed {
        if max < min {
            return Err(RandomError::value(
                "Random\\Randomizer::getFloat(): Argument #2 ($max) must be greater than or equal to argument #1 ($min)",
            ));
        }
    } else if max <= min {
        return Err(RandomError::value(
            "Random\\Randomizer::getFloat(): Argument #2 ($max) must be greater than argument #1 ($min)",
        ));
    }
    Ok(())
}

/// `Randomizer::getFloat($min, $max, $boundary)`; PHP's default boundary
/// is [`IntervalBoundary::ClosedOpen`]. `k` is always drawn with 64-bit
/// draws (two `generate()` calls for Mt19937, observed on PHP 8.5).
pub fn get_float<E: Engine + ?Sized>(
    e: &mut E,
    a: f64,
    b: f64,
    boundary: IntervalBoundary,
) -> Result<f64, RandomError> {
    check(a, b, boundary == IntervalBoundary::ClosedClosed)?;
    let g = gamma(a, b);
    let inv = 1.0 / g;
    let n = ceilint(a, b, g, inv);
    let from_b = a.abs() <= b.abs();
    Ok(match boundary {
        IntervalBoundary::ClosedClosed => {
            let hi = n;
            let k = range64(e, hi)?;
            if from_b {
                if k == hi { a } else { step(b, k, g, inv, true) }
            } else if k == hi {
                b
            } else {
                step(a, k, g, inv, false)
            }
        }
        IntervalBoundary::ClosedOpen => {
            let hi = n - 1;
            let k = range64(e, hi)?;
            if from_b {
                if k == hi {
                    a
                } else {
                    step(b, k + 1, g, inv, true)
                }
            } else {
                step(a, k, g, inv, false)
            }
        }
        IntervalBoundary::OpenClosed => {
            let hi = n - 1;
            let k = range64(e, hi)?;
            if from_b {
                step(b, k, g, inv, true)
            } else if k == hi {
                b
            } else {
                step(a, k + 1, g, inv, false)
            }
        }
        IntervalBoundary::OpenOpen => {
            if n < 2 {
                return Err(RandomError::value(
                    "The given interval is empty, there are no floats between argument #1 ($min) and argument #2 ($max)",
                ));
            }
            let hi = n - 2;
            let k = range64(e, hi)?;
            if from_b {
                step(b, k + 1, g, inv, true)
            } else {
                step(a, k + 1, g, inv, false)
            }
        }
    })
}
