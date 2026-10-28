//! Sunrise, sunset and twilight: `date_sun_info()`, `date_sunrise()` and
//! `date_sunset()`.
//!
//! The solar model is the published rise/set method of Paul Schlyter
//! ("How to compute planetary positions" and "Computing rise and set times",
//! stjarnhimlen.se/comp/ppcomp.html and riset.html): the Sun's mean orbital
//! elements at day `d` since 2000 Jan 0.0 UT, its RA/declination at the
//! local noon of the date, the time of its meridian transit, and the hour
//! angle at which it crosses a given altitude. PHP's results follow this
//! method rather than NOAA's equations: on PHP 8.5 every integer timestamp
//! and every `SUNFUNCS_RET_DOUBLE` value of the 3777 PHP 8.5 results in
//! `tests/fixtures/sun_php85.txt` matches it to the last bit (random and edge cases).
//!
//! Conventions observed on PHP 8.5 (and in ext/date/tests):
//! - "the day" is the calendar date of `$timestamp` in the default time
//!   zone; the computation is centred on the local *solar* noon of that date
//!   at the given longitude (UT 12h − longitude/15), whatever the time of day
//!   of `$timestamp` and whatever the zone (only its date matters). Results
//!   are UT hours from 00:00 UT of that calendar date.
//! - `date_sun_info()`: sunrise/sunset at altitude −35′ for the Sun's upper
//!   limb (the apparent radius 0.2666°/r is subtracted), twilights at −6°,
//!   −12° and −18° for the centre; `transit` is always a timestamp.
//! - `date_sunrise()`/`date_sunset()`: altitude 90° − `$zenith`, upper limb.
//! - Timestamps are the double `midnight + hours·3600` truncated toward zero;
//!   a NaN or out-of-range value becomes `PHP_INT_MIN` (very distant dates
//!   make the orbital model produce NaN, e.g. year 31 million or `PHP_INT_MIN`).

use crate::civil;

/// `ini date.default_latitude` default (PHP manual: date runtime configuration).
pub const DEFAULT_LATITUDE: f64 = 31.7667;
/// `ini date.default_longitude` default.
pub const DEFAULT_LONGITUDE: f64 = 35.2333;
/// `ini date.sunrise_zenith` / `date.sunset_zenith` default (observed on PHP 8.5:
/// `ini_get()` returns `"90.833333"` for both).
pub const DEFAULT_ZENITH: f64 = 90.833333;

/// Text of the `E_DEPRECATED` raised by calling `date_sunrise()` (observed on
/// PHP 8.5; test ext/date/tests/date_sunrise_and_sunset_basic.phpt).
pub const DEPRECATED_SUNRISE: &str =
    "Function date_sunrise() is deprecated since 8.1, use date_sun_info() instead";
/// Same for `date_sunset()`.
pub const DEPRECATED_SUNSET: &str =
    "Function date_sunset() is deprecated since 8.1, use date_sun_info() instead";
/// `E_DEPRECATED` text for using one of the `SUNFUNCS_RET_*` constants, with
/// `{}` the constant name (test ext/date/tests/gh14732.phpt).
pub const DEPRECATED_CONSTANT: &str = "Constant {} is deprecated since 8.4, as date_sunrise() and date_sunset() were deprecated in 8.1";

/// One `date_sun_info()` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SunTime {
    /// The event happens at this Unix timestamp (PHP `int`).
    At(i64),
    /// The Sun stays above the altitude all day (PHP `true`).
    AlwaysAbove,
    /// The Sun stays below the altitude all day (PHP `false`).
    AlwaysBelow,
}

/// The array `date_sun_info()` returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SunInfo {
    pub sunrise: SunTime,
    pub sunset: SunTime,
    pub transit: SunTime,
    pub civil_twilight_begin: SunTime,
    pub civil_twilight_end: SunTime,
    pub nautical_twilight_begin: SunTime,
    pub nautical_twilight_end: SunTime,
    pub astronomical_twilight_begin: SunTime,
    pub astronomical_twilight_end: SunTime,
}

impl SunInfo {
    /// The array's keys in PHP's order.
    pub const KEYS: [&'static str; 9] = [
        "sunrise",
        "sunset",
        "transit",
        "civil_twilight_begin",
        "civil_twilight_end",
        "nautical_twilight_begin",
        "nautical_twilight_end",
        "astronomical_twilight_begin",
        "astronomical_twilight_end",
    ];

    /// (key, value) pairs in PHP's order.
    pub fn entries(&self) -> [(&'static str, SunTime); 9] {
        let v = [
            self.sunrise,
            self.sunset,
            self.transit,
            self.civil_twilight_begin,
            self.civil_twilight_end,
            self.nautical_twilight_begin,
            self.nautical_twilight_end,
            self.astronomical_twilight_begin,
            self.astronomical_twilight_end,
        ];
        std::array::from_fn(|i| (Self::KEYS[i], v[i]))
    }
}

/// A `ValueError` to throw; the string is the full `getMessage()` text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SunError(pub String);

/// `$returnFormat` of `date_sunrise()` / `date_sunset()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SunFormat {
    /// `SUNFUNCS_RET_TIMESTAMP` (0).
    Timestamp,
    /// `SUNFUNCS_RET_STRING` (1): `"HH:MM"`.
    String,
    /// `SUNFUNCS_RET_DOUBLE` (2): hours.
    Double,
}

impl SunFormat {
    /// The constant's value as PHP defines it.
    pub fn from_int(v: i64) -> Option<SunFormat> {
        match v {
            0 => Some(SunFormat::Timestamp),
            1 => Some(SunFormat::String),
            2 => Some(SunFormat::Double),
            _ => None,
        }
    }
}

/// The value `date_sunrise()` / `date_sunset()` return.
#[derive(Debug, Clone, PartialEq)]
pub enum SunResult {
    Timestamp(i64),
    String(String),
    Double(f64),
    /// PHP `false`: polar day/night, a non-finite latitude, longitude or
    /// result.
    False,
}

/// Which event `date_sunrise()` / `date_sunset()` computes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiseSet {
    Sunrise,
    Sunset,
}

impl RiseSet {
    fn func(self) -> &'static str {
        match self {
            RiseSet::Sunrise => "date_sunrise",
            RiseSet::Sunset => "date_sunset",
        }
    }
}

// --- the solar model -------------------------------------------------------

const DEGRAD: f64 = std::f64::consts::PI / 180.0;
const RADEG: f64 = 180.0 / std::f64::consts::PI;

#[inline]
fn sind(x: f64) -> f64 {
    (x * DEGRAD).sin()
}

#[inline]
fn cosd(x: f64) -> f64 {
    (x * DEGRAD).cos()
}

#[inline]
fn atan2d(y: f64, x: f64) -> f64 {
    RADEG * y.atan2(x)
}

#[inline]
fn acosd(x: f64) -> f64 {
    RADEG * x.acos()
}

/// Reduce an angle to [0, 360).
#[inline]
fn revolution(x: f64) -> f64 {
    x - (x / 360.0).floor() * 360.0
}

/// Reduce an angle to [-180, 180).
#[inline]
fn rev180(x: f64) -> f64 {
    x - 360.0 * (x / 360.0 + 0.5).floor()
}

/// Greenwich mean sidereal time at 0h UT, in degrees (Schlyter: GMST0 is
/// the Sun's mean longitude + 180°).
fn gmst0(d: f64) -> f64 {
    revolution((180.0 + 356.0470 + 282.9404) + (0.9856002585 + 4.70935e-5) * d)
}

/// Sun's ecliptic longitude (degrees) and distance (AU) at day `d`.
fn sun_position(d: f64) -> (f64, f64) {
    let m = revolution(356.0470 + 0.9856002585 * d); // mean anomaly
    let w = 282.9404 + 4.70935e-5 * d; // argument of perihelion
    let e = 0.016709 - 1.151e-9 * d; // eccentricity
    let ea = m + e * RADEG * sind(m) * (1.0 + e * cosd(m)); // eccentric anomaly
    let x = cosd(ea) - e;
    let y = (1.0 - e * e).sqrt() * sind(ea);
    let r = (x * x + y * y).sqrt();
    let v = atan2d(y, x);
    let mut lon = v + w;
    if lon >= 360.0 {
        lon -= 360.0;
    }
    (lon, r)
}

/// Sun's right ascension, declination (degrees) and distance at day `d`.
fn sun_ra_dec(d: f64) -> (f64, f64, f64) {
    let (lon, r) = sun_position(d);
    let x = r * cosd(lon);
    let y = r * sind(lon);
    let obl = 23.4393 - 3.563e-7 * d;
    let z = y * sind(obl);
    let y = y * cosd(obl);
    (atan2d(y, x), atan2d(z, (x * x + y * y).sqrt()), r)
}

/// Rise/set of the Sun across `altitude` on day `days2000` (days since
/// 2000 Jan 0.0 UT of the calendar date).
#[derive(Debug, Clone, Copy)]
struct Crossing {
    /// Meridian transit, UT hours from 00:00 UT of the date.
    transit: f64,
    /// Rise and set (UT hours), or the polar case.
    times: Result<(f64, f64), SunTime>,
}

fn crossing(days2000: i64, lon: f64, lat: f64, mut altitude: f64, upper_limb: bool) -> Crossing {
    // local noon of the date at this longitude
    let d = days2000 as f64 + 0.5 - lon / 360.0;
    let sidtime = revolution(gmst0(d) + 180.0 + lon);
    let (ra, dec, r) = sun_ra_dec(d);
    let transit = 12.0 - rev180(sidtime - ra) / 15.0;
    if upper_limb {
        altitude -= 0.2666 / r;
    }
    let cost = (sind(altitude) - sind(lat) * sind(dec)) / (cosd(lat) * cosd(dec));
    let times = if cost >= 1.0 {
        Err(SunTime::AlwaysBelow)
    } else if cost <= -1.0 {
        Err(SunTime::AlwaysAbove)
    } else {
        // NaN (non-finite zenith, distant dates) lands here, as on PHP
        let t = acosd(cost) / 15.0;
        Ok((transit - t, transit + t))
    };
    Crossing { transit, times }
}

/// Days since 1999-12-31 (Schlyter's "2000 Jan 0.0") and the Unix time of
/// 00:00 UT of the calendar date of `ts` shifted by `local_offset` seconds.
fn local_day(ts: i64, local_offset: i64) -> (i64, f64) {
    let local = ts as i128 + local_offset as i128;
    let local = local.clamp(i64::MIN as i128, i64::MAX as i128) as i64;
    let (days, _) = civil::split_ts(local);
    // 2000-01-01 is day 10957; the exact proleptic count matches PHP far
    // outside 1900-2100 (observed on PHP 8.5, years -1000 to 33658)
    let d2000 = days - 10_956;
    (d2000, (days as i128 * civil::SECS_PER_DAY as i128) as f64)
}

/// Double → PHP integer timestamp: truncation, NaN/out of range → `i64::MIN`
/// (observed on PHP 8.5 with a NaN zenith and far dates).
fn to_ts(x: f64) -> i64 {
    const LIMIT: f64 = -(i64::MIN as f64); // 2^63
    if (-LIMIT..LIMIT).contains(&x) {
        x as i64
    } else {
        i64::MIN
    }
}

fn at(midnight: f64, hours: f64) -> SunTime {
    SunTime::At(to_ts(midnight + hours * 3600.0))
}

/// `date_sun_info($timestamp, $latitude, $longitude)`.
///
/// `local_offset` is the default time zone's UTC offset (seconds) at
/// `ts`: it only picks the calendar date.
pub fn sun_info(ts: i64, lat: f64, lon: f64, local_offset: i64) -> Result<SunInfo, SunError> {
    // test ext/date/tests/gh14732.phpt
    if !lat.is_finite() {
        return Err(SunError(
            "date_sun_info(): Argument #2 ($latitude) must be finite".into(),
        ));
    }
    if !lon.is_finite() {
        return Err(SunError(
            "date_sun_info(): Argument #3 ($longitude) must be finite".into(),
        ));
    }
    let (d2000, midnight) = local_day(ts, local_offset);
    let pair = |altitude: f64, limb: bool| -> (SunTime, SunTime, f64) {
        let c = crossing(d2000, lon, lat, altitude, limb);
        match c.times {
            Ok((rise, set)) => (at(midnight, rise), at(midnight, set), c.transit),
            Err(polar) => (polar, polar, c.transit),
        }
    };
    let (sunrise, sunset, transit) = pair(-35.0 / 60.0, true);
    let (civil_twilight_begin, civil_twilight_end, _) = pair(-6.0, false);
    let (nautical_twilight_begin, nautical_twilight_end, _) = pair(-12.0, false);
    let (astronomical_twilight_begin, astronomical_twilight_end, _) = pair(-18.0, false);
    Ok(SunInfo {
        sunrise,
        sunset,
        transit: at(midnight, transit),
        civil_twilight_begin,
        civil_twilight_end,
        nautical_twilight_begin,
        nautical_twilight_end,
        astronomical_twilight_begin,
        astronomical_twilight_end,
    })
}

/// The default `$utcOffset` (hours) of `date_sunrise()` / `date_sunset()`:
/// pass the default time zone's UTC offset in seconds **at Unix time 0**.
///
/// Observed on PHP 8.5: the default is not the offset at `$timestamp` but
/// the zone's offset at 1970-01-01T00:00:00Z, DST included (Europe/London
/// gives +1 all year, from British Standard Time 1968-71; Australia/Hobart
/// +11; Pacific/Kiritimati −10:40; Asia/Kathmandu +5:30; America/Caracas −4;
/// Africa/Monrovia −0:44:30).
pub fn default_utc_offset_hours(offset_at_epoch: i64) -> f64 {
    offset_at_epoch as f64 / 3600.0
}

/// `date_sunrise()` / `date_sunset()`.
///
/// - `format`: the raw `$returnFormat` integer.
/// - `lat`, `lon`, `zenith`: the arguments, or the ini defaults when null.
/// - `utc_offset`: `$utcOffset` in hours, or [`default_utc_offset_hours`]
///   when null. Only the string and double results use it.
/// - `local_offset`: the default zone's UTC offset (seconds) at `ts`; it
///   picks the calendar date (observed on PHP 8.5: the date never depends on
///   `$utcOffset`).
///
/// The deprecation notices are the caller's ([`DEPRECATED_SUNRISE`] …).
#[allow(clippy::too_many_arguments)]
pub fn sun_rise_set(
    which: RiseSet,
    ts: i64,
    format: i64,
    lat: f64,
    lon: f64,
    zenith: f64,
    utc_offset: f64,
    local_offset: i64,
) -> Result<SunResult, SunError> {
    // test ext/date/tests/date_sunrise_and_sunset_error.phpt
    let Some(format) = SunFormat::from_int(format) else {
        return Err(SunError(format!(
            "{}(): Argument #2 ($returnFormat) must be one of SUNFUNCS_RET_TIMESTAMP, \
             SUNFUNCS_RET_STRING, or SUNFUNCS_RET_DOUBLE",
            which.func()
        )));
    };
    // test ext/date/tests/gh14732.phpt: false, not an error, here
    if !lat.is_finite() || !lon.is_finite() {
        return Ok(SunResult::False);
    }
    let (d2000, midnight) = local_day(ts, local_offset);
    let c = crossing(d2000, lon, lat, 90.0 - zenith, true);
    let Ok((rise, set)) = c.times else {
        // polar day and polar night alike (date_sunrise_variation8.phpt)
        return Ok(SunResult::False);
    };
    let hours = match which {
        RiseSet::Sunrise => rise,
        RiseSet::Sunset => set,
    };
    if format == SunFormat::Timestamp {
        return Ok(SunResult::Timestamp(to_ts(midnight + hours * 3600.0)));
    }
    // Local hours wrapped into [0, 24). This form (rather than fmod) also
    // makes ±PHP_FLOAT_MAX offsets non-finite (test gh16454.phpt) while 1e300
    // gives 0.0 (observed on PHP 8.5).
    let mut n = hours + utc_offset;
    n -= (n / 24.0).floor() * 24.0;
    if !n.is_finite() {
        // NaN/INF offsets and NaN zeniths (gh18481.phpt, observed on PHP 8.5)
        return Ok(SunResult::False);
    }
    Ok(match format {
        SunFormat::Double => SunResult::Double(n),
        _ => {
            // both fields truncated (observed on PHP 8.5: 5.9298 → "05:55")
            let h = n as i64;
            let m = ((n - h as f64) * 60.0) as i64;
            SunResult::String(format!("{h:02}:{m:02}"))
        }
    })
}

/// `date_sunrise()`; see [`sun_rise_set`].
pub fn sunrise(
    ts: i64,
    format: i64,
    lat: f64,
    lon: f64,
    zenith: f64,
    utc_offset: f64,
    local_offset: i64,
) -> Result<SunResult, SunError> {
    sun_rise_set(
        RiseSet::Sunrise,
        ts,
        format,
        lat,
        lon,
        zenith,
        utc_offset,
        local_offset,
    )
}

/// `date_sunset()`; see [`sun_rise_set`].
pub fn sunset(
    ts: i64,
    format: i64,
    lat: f64,
    lon: f64,
    zenith: f64,
    utc_offset: f64,
    local_offset: i64,
) -> Result<SunResult, SunError> {
    sun_rise_set(
        RiseSet::Sunset,
        ts,
        format,
        lat,
        lon,
        zenith,
        utc_offset,
        local_offset,
    )
}
