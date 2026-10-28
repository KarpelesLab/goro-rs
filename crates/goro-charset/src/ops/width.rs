//! Display width (`mb_strwidth`, `mb_strimwidth`): East Asian Wide and
//! Fullwidth characters count 2, everything else 1 (PHP manual:
//! mb_strwidth; test ext/mbstring/tests/unicode_versions.phpt). The East
//! Asian Width property comes from the intl crate.

use intl::unicode::{EastAsianWidth, east_asian_width_u32};

/// The width of one code point.
#[inline]
pub fn cp_width(cp: u32) -> usize {
    if cp < 0x1100 {
        return 1;
    }
    match east_asian_width_u32(cp) {
        EastAsianWidth::Wide | EastAsianWidth::Fullwidth => 2,
        _ => 1,
    }
}

/// The width of code points.
pub fn cps_width(cps: &[u32]) -> usize {
    cps.iter().map(|&c| cp_width(c)).sum()
}
